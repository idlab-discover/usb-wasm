//! UVC webcam capture via USB isochronous transfers.
//!
//! This module implements a minimal UVC (USB Video Class) capture pipeline:
//!   1. Enumerate USB devices and locate a UVC Video Streaming interface.
//!   2. Select the alternate setting with the highest isochronous bandwidth.
//!   3. Perform the UVC Probe/Commit handshake to negotiate stream parameters.
//!   4. Issue isochronous IN transfers and reassemble individual video frames
//!      by tracking UVC payload header flags (FID toggle, EOF bit) and, for
//!      MJPEG streams, the JPEG End-Of-Image marker (0xFF 0xD9).
//!   5. Save each complete frame as frame.png and render it as ASCII art.
//!
//! Supported pixel formats: MJPEG and YUYV (YUV 4:2:2 packed).
//!
//! Requirements:
//!   - A host runtime that implements USB isochronous transfers via WASI.
//!   - Any UVC-compliant webcam (USB device class 0x0E or multi-function 0xEF).

use anyhow::{anyhow, Context, Result};
// use image::GenericImageView; // Removed as it is unused with the concrete RgbImage type
use std::io::{self, BufRead, Write};

// WIT bindings are generated in lib.rs via wit_bindgen::generate!.
// On native targets (non-WASM) the bindings are gated behind #[cfg(target_family = "wasm")]
// in lib.rs, so we reference the pre-built usb-wasm-bindings crate directly here.
use usb_wasm_bindings::component::usb::{
    device::{list_devices, UsbDevice},
    transfers::{await_iso_transfer, await_transfer, TransferOptions, TransferSetup, TransferType},
};

// ── UVC class constants ───────────────────────────────────────────────────────

/// USB device/interface class code for Video devices (UVC).
const USB_CLASS_VIDEO: u8 = 0x0E;

/// UVC subclass for the Video Streaming interface (carries actual pixel data).
/// The Video Control interface (subclass 0x01) handles camera controls only.
const USB_SUBCLASS_VIDEO_STREAMING: u8 = 0x02;

// ── ASCII rendering ───────────────────────────────────────────────────────────

/// Luminance ramp used when converting pixels to ASCII characters.
/// Ordered from darkest (space) to brightest (@).
const ASCII_CHARS: &[char] = &[' ', '.', ',', '-', '~', ':', ';', '=', '!', '*', '#', '$', '@'];

// ── Interface / endpoint selection ───────────────────────────────────────────

/// Scan all interfaces in the active configuration and return the Video Streaming
/// alternate setting that offers the largest isochronous IN endpoint.
///
/// UVC cameras expose the zero-bandwidth alt setting (alt 0) by default.
/// Actual streaming requires switching to an alternate setting with a non-zero
/// isochronous endpoint. We pick the one with the highest effective packet size
/// so we get maximum throughput with a single alternate setting switch.
///
/// Returns `(interface_number, alternate_setting, endpoint_address, max_packet_size)`.
fn find_best_streaming_interface(device: &UsbDevice) -> Result<(u8, u8, u8, u16)> {
    let config_desc = device
        .get_active_configuration_descriptor()
        .map_err(|e| anyhow!("{:?}", e))
        .context("Failed to get active configuration")?;

    let mut best: Option<(u8, u8, u8, u16)> = None;

    for iface in &config_desc.interfaces {
        if iface.interface_class != USB_CLASS_VIDEO
            || iface.interface_subclass != USB_SUBCLASS_VIDEO_STREAMING
        {
            continue;
        }

        for ep in &iface.endpoints {
            // We need an IN endpoint (bit 7 set) with transfer type isochronous (bits 1:0 == 1).
            let is_iso_in = (ep.endpoint_address & 0x80 != 0) && (ep.attributes & 0x03 == 1);
            if !is_iso_in {
                continue;
            }

            // wMaxPacketSize encodes the per-transaction size in bits 10:0 and a
            // high-bandwidth multiplier in bits 12:11 (USB 2.0 spec, Table 9-13).
            let base_size  = ep.max_packet_size & 0x7FF;
            let multiplier = 1 + ((ep.max_packet_size >> 11) & 0x03);
            let effective_size = base_size * multiplier;

            if best.map_or(true, |(_, _, _, s)| effective_size > s) {
                best = Some((
                    iface.interface_number,
                    iface.alternate_setting,
                    ep.endpoint_address,
                    effective_size,
                ));
            }
        }
    }

    best.ok_or_else(|| anyhow!("No UVC streaming interface found"))
}

// ── UVC payload header parsing ────────────────────────────────────────────────

/// Parse the two-byte UVC payload header that precedes every isochronous packet.
///
/// Layout (UVC 1.5 spec, section 2.4.3.3):
///   Byte 0: HLE  - Header Length (includes these two bytes)
///   Byte 1: BFH  - Bit Field Header flags
///     bit 0: FID  - Frame Identifier, toggles at every new frame boundary
///     bit 1: EOF  - End of Frame, set on the last packet of a frame
///     bit 2: PTS  - Presentation Time Stamp present
///     bit 3: SCR  - Source Clock Reference present
///     bit 6: ERR  - Device error condition
///     bit 7: EOH  - End of BFH header (always 1)
///
/// Returns `(header_length_in_bytes, end_of_frame_flag)`.
/// Returns `(0, false)` if the header is malformed.
fn parse_payload_header(data: &[u8]) -> (usize, bool) {
    if data.len() < 2 {
        return (0, false);
    }
    let header_len = data[0] as usize;
    // Sanity-check: header must be at least 2 bytes and fit inside the packet.
    if header_len < 2 || header_len > data.len() {
        return (0, false);
    }
    let end_of_frame = (data[1] & 0x02) != 0; // EOF bit (bit 1 of BFH)
    (header_len, end_of_frame)
}

// ── Pixel conversion helpers ──────────────────────────────────────────────────

/// Convert an RGB triple to an ASCII character using BT.601 luma weighting.
///
/// The luma value is mapped linearly onto the ASCII_CHARS ramp so that
/// dark pixels become spaces and bright pixels become '@'.
fn rgb_to_ascii(r: u8, g: u8, b: u8) -> char {
    let luma = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8;
    let idx  = (luma as usize * (ASCII_CHARS.len() - 1)) / 255;
    ASCII_CHARS[idx]
}

// ── Frame persistence ─────────────────────────────────────────────────────────

/// Decode an MJPEG frame and correct the color channels.
///
/// Many UVC cameras (including the Logitech Brio series) output MJPEG without a
/// proper JFIF colorspace marker, or with the Cb and Cr chroma channels in the
/// wrong order relative to what the JPEG spec expects. The `image` crate decodes
/// the JPEG correctly given the header it receives, but the resulting RGB image
/// has R and B swapped compared to the true scene colors, producing a purple/
/// magenta cast on all frames.
///
/// This function decodes the JPEG and then swaps the R and B channels in every
/// pixel to restore the correct colors. If a future camera does NOT show a purple
/// tint without this swap, remove the channel-swap loop.
fn decode_mjpeg(data: &[u8]) -> Result<image::RgbImage> {
    let img = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
        .context("MJPEG decode failed")?;
    let mut rgb = img.into_rgb8();

    // Swap R (index 0) and B (index 2) to correct the Cb/Cr channel ordering
    // that the camera encodes relative to what the JPEG decoder assumes.
    for pixel in rgb.pixels_mut() {
        pixel.0.swap(0, 2);
    }

    Ok(rgb)
}

/// Save a captured frame to "frame.png" in the current working directory.
///
/// Two pixel formats are handled:
///   - MJPEG: detected by the 0xFF 0xD8 SOI marker; decoded via decode_mjpeg().
///   - YUYV:  YUV 4:2:2 packed; each 4-byte macro-pixel encodes two RGB pixels.
///
/// For YUYV frames the RGB buffer is padded with black pixels if the raw data is
/// slightly shorter than expected (within the 5% tolerance of match_resolution),
/// ensuring ImageBuffer::from_raw never receives an undersized slice.
fn save_as_png(frame_data: &[u8], width: u32, height: u32) -> Result<()> {
    use image::{ImageBuffer, Rgb};

    if frame_data.starts_with(&[0xff, 0xd8]) {
        // MJPEG path: decode and correct colors, then save as PNG.
        let rgb = decode_mjpeg(frame_data)?;
        rgb.save("frame.png")?;
        println!("Saved frame.png (MJPEG, {}x{})", width, height);
    } else {
        // YUYV path: manual YUV -> RGB conversion.
        // Each group of 4 bytes [Y0, U, Y1, V] represents two horizontally
        // adjacent pixels sharing a single U/V chroma pair (4:2:2 sampling).
        let mut rgb_buf = Vec::with_capacity((width * height * 3) as usize);

        for i in 0..(frame_data.len() / 4) {
            /*let y0 = frame_data[i * 4]     as f32;
            let u  = frame_data[i * 4 + 1] as f32 - 128.0;
            let y1 = frame_data[i * 4 + 2] as f32;
            let v  = frame_data[i * 4 + 3] as f32 - 128.0;*/

            // BT.601 full-range YUV -> RGB coefficients.
            let yuv_to_rgb = |y: f32, u: f32, v: f32| -> [u8; 3] {
                let r = (y + 1.402       * v).clamp(0.0, 255.0) as u8;
                let g = (y - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8;
                let b = (y + 1.772       * u).clamp(0.0, 255.0) as u8;
                [r, g, b]
            };

            rgb_buf.extend_from_slice(&yuv_to_rgb(y0, u, v));
            rgb_buf.extend_from_slice(&yuv_to_rgb(y1, u, v));
        }

        let expected_len = (width * height * 3) as usize;

        // Clamp the buffer to exactly the expected size.
        // Truncate if we got slightly more data than expected, or pad with black
        // pixels if we got slightly less (within the 5% match_resolution window).
        if rgb_buf.len() > expected_len {
            rgb_buf.truncate(expected_len);
        } else if rgb_buf.len() < expected_len {
            rgb_buf.resize(expected_len, 0u8);
        }

        match ImageBuffer::<Rgb<u8>, _>::from_raw(width, height, rgb_buf) {
            Some(buf) => match buf.save("frame.png") {
                Ok(_)  => println!("Saved frame.png (YUYV, {}x{})", width, height),
                Err(e) => eprintln!("Failed to save frame.png: {:?}", e),
            },
            None => eprintln!(
                "Failed to create ImageBuffer (expected {} bytes, {}x{})",
                expected_len, width, height
            ),
        }
    }

    Ok(())
}

// ── YUYV resolution detection ─────────────────────────────────────────────────

/// Common YUYV resolutions. Each entry is (width, height); the raw byte size
/// for YUYV is width * height * 2 (2 bytes per pixel in 4:2:2 packing).
const KNOWN_RESOLUTIONS: &[(u32, u32)] = &[
    (640, 480),
    (352, 288),
    (320, 240),
    (320, 180),
    (320, 160),
    (320, 120),
    (176, 144),
    (160, 144),
    (160, 120),
    (160,  90),
];

/// Try to identify the resolution of a YUYV frame from its byte length.
///
/// Accepts any resolution from KNOWN_RESOLUTIONS whose expected size is within
/// 5% of `frame_size`, and returns the closest match. Returns None if no
/// known resolution matches (which usually means the frame is incomplete).
fn match_resolution(frame_size: usize) -> Option<(u32, u32)> {
    let mut best: Option<(u32, u32, usize)> = None; // (width, height, abs_diff)

    for &(w, h) in KNOWN_RESOLUTIONS {
        let expected = (w * h * 2) as usize;
        let diff     = frame_size.abs_diff(expected);

        // Accept if the deviation is less than 5% of the expected size.
        if diff * 20 <= expected && best.map_or(true, |(_, _, d)| diff < d) {
            best = Some((w, h, diff));
        }
    }

    best.map(|(w, h, _)| (w, h))
}

/// Resolve width/height from a YUYV frame size, falling back to coarse estimates
/// when the size does not match any known resolution exactly.
fn get_resolution(frame_size: usize, _negotiated_size: u32) -> (u32, u32) {
    match_resolution(frame_size).unwrap_or_else(|| {
        // Rough fallback based on minimum byte thresholds for common resolutions.
        if      frame_size >= 153_600 { (320, 240) }
        else if frame_size >= 102_400 { (320, 160) }
        else if frame_size >= 76_800  { (320, 120) }
        else if frame_size >= 50_688  { (176, 144) }
        else if frame_size >= 46_080  { (160, 144) }
        else                          { (160, 120) }
    })
}

// ── ASCII rendering ───────────────────────────────────────────────────────────

/// Decode a complete video frame and render it as an 80-column ASCII art string.
///
/// For MJPEG the image crate handles decoding; for YUYV only the Y (luma) plane
/// is sampled directly, which avoids a full YUV->RGB conversion at render time.
///
/// The output is scaled to 80 columns with a 0.5 aspect-ratio correction so that
/// the image does not appear stretched in a standard terminal font.
fn process_frame(frame_data: &[u8], negotiated_size: u32) -> Result<String> {
    // Decode MJPEG once and reuse the result for both dimension detection and pixel
    // sampling. We use RgbImage directly rather than DynamicImage to avoid WASM32
    // alignment traps: DynamicImage is a large enum whose variants require strict
    // alignment that the WASM32 stack allocator does not always guarantee.
    let maybe_img: Option<image::RgbImage> = if frame_data.starts_with(&[0xff, 0xd8]) {
        Some(decode_mjpeg(frame_data).context("Failed to decode JPEG frame")?)
    } else {
        None
    };

    let (width, height) = match &maybe_img {
        Some(img) => (img.width(), img.height()),
        None      => get_resolution(frame_data.len(), negotiated_size),
    };

    let target_width  = 80u32;
    // The 0.5 factor compensates for terminal characters being roughly twice as
    // tall as they are wide, which would otherwise make the image appear squashed.
    let target_height = (height as f32 * (target_width as f32 / width as f32) * 0.5) as u32;

    let x_step = width  as f32 / target_width  as f32;
    let y_step = height as f32 / target_height as f32;

    let mut ascii = String::with_capacity(((target_width + 1) * target_height) as usize);

    for ty in 0..target_height {
        for tx in 0..target_width {
            // Sample the nearest source pixel (nearest-neighbour, no interpolation).
            let x = (tx as f32 * x_step) as u32;
            let y = (ty as f32 * y_step) as u32;

            if let Some(ref img) = maybe_img {
                // MJPEG: read the color-corrected RGB pixel.
                // RgbImage::get_pixel returns Rgb<u8>; .0 is [u8; 3].
                let pixel = img.get_pixel(x.min(width - 1), y.min(height - 1));
                let image::Rgb([r, g, b]) = *pixel;
                ascii.push(rgb_to_ascii(r, g, b));
            } else {
                // YUYV: byte 0 of each macro-pixel is the Y0 luma value.
                // Pixel at (x, y) has its luma byte at index (y * width + x) * 2.
                let byte_idx = (y * width + x) as usize * 2;
                if byte_idx < frame_data.len() {
                    let luma     = frame_data[byte_idx];
                    let char_idx = (luma as usize * (ASCII_CHARS.len() - 1)) / 255;
                    ascii.push(ASCII_CHARS[char_idx]);
                } else {
                    ascii.push(' ');
                }
            }
        }
        ascii.push('\n');
    }

    Ok(ascii)
}

// ── Frame emission ────────────────────────────────────────────────────────────

/// Validate, save, and display a completed frame.
///
/// Shared by both the main capture loop and the final buffer flush so that frame
/// handling logic is not duplicated.
///
/// Validation rules:
///   - Frames shorter than `min_frame_size` are rejected as USB fragments.
///   - YUYV frames whose size does not match any known resolution are rejected
///     as likely incomplete. MJPEG frames always pass through because the JPEG
///     decoder will catch corruption by itself.
fn emit_frame(
    complete_frame: &[u8],
    frame_count: &mut u32,
    actual_frame_size: u32,
    min_frame_size: usize,
) {
    if complete_frame.len() < min_frame_size {
        eprintln!(
            "Skipping fragment ({} bytes < minimum {} bytes)",
            complete_frame.len(), min_frame_size
        );
        return;
    }

    let is_mjpeg = complete_frame.starts_with(&[0xff, 0xd8]);

    // For YUYV, reject frames whose size does not match any known resolution.
    // MJPEG frame size varies per frame (variable compression), so we skip this check.
    if !is_mjpeg && match_resolution(complete_frame.len()).is_none() {
        eprintln!(
            "Skipping YUYV frame with unrecognized size ({} bytes) -- likely incomplete",
            complete_frame.len()
        );
        return;
    }

    *frame_count += 1;

    // Determine frame dimensions before rendering.
    // We call decode_mjpeg here (rather than a cheaper dimension-only decode) so that
    // any JPEG parse error surfaces early with a clear message before we attempt
    // save_as_png and process_frame.
    let (w, h) = if is_mjpeg {
        match decode_mjpeg(complete_frame) {
            Ok(img) => (img.width(), img.height()),
            Err(e)  => { eprintln!("JPEG decode error: {}", e); return; }
        }
    } else {
        get_resolution(complete_frame.len(), actual_frame_size)
    };

    let _ = save_as_png(complete_frame, w, h);

    match process_frame(complete_frame, actual_frame_size) {
        Ok(ascii) => {
            // ANSI escape sequences: clear screen (2J) and move cursor to top-left (H).
            print!("\x1B[2J\x1B[H");
            println!("Frame #{} ({} bytes, {}x{})\n{}", frame_count, complete_frame.len(), w, h, ascii);
            io::stdout().flush().ok();
        }
        Err(e) => eprintln!("Frame render error: {}", e),
    }
}

// ── Main entry point ──────────────────────────────────────────────────────────

pub fn run_webcam() -> Result<()> {
    println!("Starting UVC webcam capture via isochronous transfers...");

    // Enumerate all USB devices exposed by the WASI host runtime.
    let devices = list_devices().map_err(|e| anyhow!("{:?}", e))?;
    println!("Found {} USB device(s)", devices.len());

    // Select the first device that advertises UVC (class 0x0E) at the device level,
    // or the multi-function IAD class (0xEF) used by cameras like the Logitech Brio
    // that expose UVC as a function within a composite device.
    let (device, descriptor, location) = devices
        .into_iter()
        .find(|(_, desc, _)| desc.device_class == USB_CLASS_VIDEO || desc.device_class == 0xEF)
        .ok_or_else(|| anyhow!("No UVC device found (class 0x0E or 0xEF)"))?;

    println!(
        "Found UVC device: {:04x}:{:04x} at bus {} address {}",
        descriptor.vendor_id, descriptor.product_id,
        location.bus_number,  location.device_address
    );

    // Open the device and obtain an exclusive handle.
    let handle: usb_wasm_bindings::component::usb::device::DeviceHandle = device
        .open()
        .map_err(|e| anyhow!("{:?}", e))
        .context("Failed to open device")?;

    // Identify the Video Streaming interface and its best alternate setting.
    let (iface_num, alt_setting, ep_addr, max_packet_size) =
        find_best_streaming_interface(&device)?;

    println!(
        "Selected interface {}, alt setting {}, endpoint 0x{:02x}, max packet size {} bytes",
        iface_num, alt_setting, ep_addr, max_packet_size
    );

    // Claim exclusive kernel access to the streaming interface.
    handle
        .claim_interface(iface_num)
        .map_err(|e| anyhow!("{:?}", e))
        .context("Failed to claim interface")?;

    // Switch to alt setting 0 first (zero bandwidth) to ensure a clean state,
    // then switch to the target alt setting to allocate isochronous bandwidth.
    println!("Resetting interface to alt setting 0...");
    handle.set_interface_altsetting(iface_num, 0).ok();

    handle
        .set_interface_altsetting(iface_num, alt_setting)
        .map_err(|e| anyhow!("{:?}", e))
        .context("Failed to set alt setting")?;
    println!("Set alt setting {}", alt_setting);

    // ── UVC Probe/Commit handshake ────────────────────────────────────────────
    //
    // The UVC specification requires a Probe/Commit negotiation before streaming:
    //   1. GET_CUR (Probe)  -- read the camera's current streaming parameters.
    //   2. SET_CUR (Probe)  -- write back (possibly modified) parameters.
    //   3. SET_CUR (Commit) -- lock in the parameters; streaming may now begin.
    //
    // The 26-byte VideoProbeCommit control block (UVC 1.0 layout) contains fields
    // such as bFormatIndex, bFrameIndex, dwFrameInterval, and dwMaxVideoFrameSize.
    // We use dwMaxVideoFrameSize (bytes 18-21) to know the negotiated frame size.
    //
    // This handshake must happen AFTER set_interface_altsetting.

    println!("Performing UVC Probe/Commit handshake...");

    let probe_xfer = handle.new_transfer(
        TransferType::Control,
        TransferSetup {
            bm_request_type: 0xA1, // Direction: IN, Type: Class, Recipient: Interface
            b_request: 0x81,       // GET_CUR
            w_value:   0x0100,     // CS = VS_PROBE_CONTROL (1) in the high byte
            w_index:   iface_num as u16,
        },
        26, // VideoProbeCommit control block size for UVC 1.0
        TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
    ).map_err(|e| anyhow!("{:?}", e))?;

    probe_xfer.submit_transfer(&[]).map_err(|e| anyhow!("{:?}", e))?;
    let probe_data = await_transfer(probe_xfer).map_err(|e| anyhow!("{:?}", e))?;
    println!("  Probe GET_CUR: {} bytes received", probe_data.len());

    // dwMaxVideoFrameSize lives at offset 18 in the probe/commit block.
    let mut actual_frame_size = 0u32;

    if probe_data.len() >= 26 {
        actual_frame_size = u32::from_le_bytes(probe_data[18..22].try_into().unwrap());
        println!(
            "  Format index: {}, Frame index: {}, Frame interval: {} (100 ns units)",
            probe_data[2], probe_data[3],
            u32::from_le_bytes(probe_data[4..8].try_into().unwrap())
        );
        println!("  dwMaxVideoFrameSize: {} bytes", actual_frame_size);

        // Echo the probe data back with SET_CUR to confirm the parameters.
        let set_probe = handle.new_transfer(
            TransferType::Control,
            TransferSetup {
                bm_request_type: 0x21, // Direction: OUT, Type: Class, Recipient: Interface
                b_request: 0x01,       // SET_CUR
                w_value:   0x0100,     // VS_PROBE_CONTROL
                w_index:   iface_num as u16,
            },
            probe_data.len() as u32,
            TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
        ).map_err(|e| anyhow!("{:?}", e))?;
        set_probe.submit_transfer(&probe_data).map_err(|e| anyhow!("{:?}", e))?;
        await_transfer(set_probe).map_err(|e| anyhow!("{:?}", e))?;

        // Commit: lock in the agreed parameters so the camera starts streaming.
        let commit = handle.new_transfer(
            TransferType::Control,
            TransferSetup {
                bm_request_type: 0x21,
                b_request: 0x01,   // SET_CUR
                w_value:   0x0200, // CS = VS_COMMIT_CONTROL (2) in the high byte
                w_index:   iface_num as u16,
            },
            probe_data.len() as u32,
            TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
        ).map_err(|e| anyhow!("{:?}", e))?;
        commit.submit_transfer(&probe_data).map_err(|e| anyhow!("{:?}", e))?;
        await_transfer(commit).map_err(|e| anyhow!("{:?}", e))?;

        println!("Handshake complete");
    }

    // ── Isochronous transfer parameters ──────────────────────────────────────
    //
    // Each isochronous transfer groups num_packets USB micro-frames into a single
    // host buffer. The flat `data` buffer returned by await_iso_transfer has size
    // num_packets * max_packet_size; the per-packet metadata (actual_length) tells
    // us how many bytes each micro-frame actually delivered.

    let num_packets:   u32 = 32;
    let packet_stride: u32 = max_packet_size as u32; // stride between packets in flat_data
    let buffer_size:   u32 = num_packets * packet_stride;

    let opts = TransferOptions {
        endpoint:    ep_addr,
        timeout_ms:  2000,
        stream_id:   0,
        iso_packets: num_packets,
    };

    println!(
        "Transfer buffer: {} packets x {} bytes = {} KB",
        num_packets, packet_stride, buffer_size / 1024
    );

    // Minimum accepted frame size: the smallest supported YUYV resolution
    // (160x90 = 28 800 bytes). Anything smaller is a USB fragment.
    let min_frame_size: usize = 28_800;
    println!("Minimum frame size: {} bytes", min_frame_size);

    // ── Interactive capture loop ──────────────────────────────────────────────
    //
    // The loop waits for user input before capturing each frame. This makes it
    // easy to step through frames one at a time and inspect frame.png after each.
    //
    // For every ENTER press the inner loop issues isochronous transfers until one
    // complete frame has been reassembled. Frame boundaries are detected by:
    //
    //   FID toggle:  The Frame Identifier bit in the UVC payload header alternates
    //                between 0 and 1 at every frame boundary. When a toggle is
    //                detected the current packet belongs to the NEW frame, so we
    //                emit the accumulated old frame first, then start a fresh buffer
    //                with the current packet's payload.
    //
    //   EOF bit:     Bit 1 of the BFH byte signals the last packet of a frame.
    //                We append the current payload first, then emit.
    //
    //   MJPEG EOI:   A JPEG stream is also complete when it ends with 0xFF 0xD9.
    //                Checked after appending the current payload, same as EOF.

    let mut frame_buffer:  Vec<u8> = Vec::new();
    let mut frame_count:   u32     = 0;
    let mut last_fid:      u8      = 0;
    // Set to true once we have seen the JPEG SOI marker (0xFF 0xD8) at the start
    // of a payload. Until then all payloads are discarded so we never accumulate
    // the tail end of a frame that was already in progress when we started reading.
    let mut frame_started: bool;

    println!("\nInteractive mode: press ENTER to capture a frame, or type EXIT to quit.");

    let stdin = io::stdin();

    'outer: loop {
        print!("Press ENTER for frame #{} (or EXIT): ", frame_count + 1);
        io::stdout().flush().ok();

        let mut input = String::new();
        if stdin.lock().read_line(&mut input).is_err() {
            break;
        }
        if input.trim().eq_ignore_ascii_case("exit") {
            println!("Exiting.");
            break;
        }

        println!("Capturing...");
        frame_buffer.clear();
        frame_started = false; // wait for a clean SOI before accumulating
        let captured_before = frame_count;

        // Up to 2000 isochronous transfers per frame attempt. At 32 packets per
        // transfer and ~3 KB per packet this is well over 100 MB of headroom,
        // far more than the largest expected frame (~175 KB for 640x480 MJPEG).
        for _attempt in 0..2000 {
            let xfer = handle
                .new_transfer(
                    TransferType::Isochronous,
                    TransferSetup { bm_request_type: 0, b_request: 0, w_value: 0, w_index: 0 },
                    buffer_size,
                    opts,
                )
                .map_err(|e| anyhow!("{:?}", e))?;

            xfer.submit_transfer(&[]).map_err(|e| anyhow!("{:?}", e))?;
            let iso_result = await_iso_transfer(xfer).map_err(|e| anyhow!("{:?}", e))?;

            let flat_data = iso_result.data;
            let packets   = iso_result.packets;
            let mut offset = 0usize;

            for (i, pkt) in packets.iter().enumerate() {
                let actual_len = pkt.actual_length as usize;

                if actual_len == 0 {
                    // Empty micro-frame: no data token was issued. Advance the stride.
                    offset += packet_stride as usize;
                    continue;
                }

                // Slice out this packet from the flat buffer using the fixed stride.
                // actual_len <= packet_stride is guaranteed by the host runtime.
                let pkt_data = &flat_data[offset..offset + actual_len];
                offset += packet_stride as usize;

                let (hdr_len, is_eof) = parse_payload_header(pkt_data);
                if hdr_len == 0 {
                    // Malformed or zero-length UVC header; skip this packet.
                    continue;
                }

                // Log the raw header bytes for the first two packets of the first
                // transfer so the FID/EOF bit pattern is visible in diagnostics.
                if frame_count == captured_before && i < 2 {
                    println!(
                        "[DIAG] packet {}: HLE=0x{:02x} BFH=0b{:08b} length={}",
                        i, pkt_data[0], pkt_data[1], actual_len
                    );
                }

                let fid     = pkt_data[1] & 0x01;
                let payload = if hdr_len < pkt_data.len() { &pkt_data[hdr_len..] } else { &[][..] };

                // ── Frame boundary detection ──────────────────────────────────
                //
                // Two distinct cases with different payload ordering:
                //
                // Case A -- FID toggle:
                //   The current packet is the FIRST packet of the next frame.
                //   Emit the accumulated old frame WITHOUT this payload, then
                //   start the new buffer with this payload.
                //
                // Case B -- EOF bit or MJPEG EOI:
                //   The current packet is the LAST packet of the current frame.
                //   Append this payload first so it is included, then emit.
                //   (MJPEG EOI must be checked AFTER appending so 0xFF 0xD9 is visible.)

                let fid_toggle = !frame_buffer.is_empty() && fid != last_fid;

                if fid_toggle {
                    // Case A: emit old frame, then start new frame with current payload.
                    if frame_started {
                        println!(
                            "[DIAG] FID toggle -> emitting frame #{} ({} bytes)",
                            frame_count + 1, frame_buffer.len()
                        );
                        let complete_frame = std::mem::take(&mut frame_buffer);
                        emit_frame(&complete_frame, &mut frame_count, actual_frame_size, min_frame_size);
                    } else {
                        // We never found a clean SOI for this "frame" — discard it.
                        println!("[DIAG] FID toggle, no SOI seen yet — discarding {} bytes", frame_buffer.len());
                        frame_buffer.clear();
                    }

                    // Begin the new frame only if the incoming payload starts with SOI.
                    // In UVC/MJPEG the very first payload byte after the header at a
                    // frame boundary is always 0xFF 0xD8. If it is not, the camera is
                    // in an unexpected state and we wait for the next clean start.
                    if payload.starts_with(&[0xff, 0xd8]) {
                        frame_buffer.extend_from_slice(payload);
                        frame_started = true;
                    } else {
                        frame_started = false;
                        println!("[DIAG] FID toggle payload does not start with SOI (0x{:02x} 0x{:02x}) — waiting",
                            payload.first().copied().unwrap_or(0),
                            payload.get(1).copied().unwrap_or(0));
                    }

                    if frame_count > captured_before {
                        last_fid = fid;
                        continue 'outer;
                    }
                } else {
                    // Case B: append payload, then check for frame completion.
                    //
                    // If we have not yet seen an SOI marker, check whether this payload
                    // contains one. This handles the case where we start reading mid-stream
                    // and the camera has not yet sent a FID toggle to signal a new frame.
                    if !frame_started {
                        if let Some(soi_pos) = payload.windows(2).position(|w| w == [0xff, 0xd8]) {
                            // Found SOI — start accumulating from this byte onwards.
                            frame_buffer.extend_from_slice(&payload[soi_pos..]);
                            frame_started = true;
                            println!("[DIAG] SOI found at offset {} in payload — frame accumulation started", soi_pos);
                        }
                        // else: still no SOI in sight; skip this payload entirely.
                    } else {
                        frame_buffer.extend_from_slice(payload);
                    }

                    // Only check for completion when a valid SOI-anchored frame is in progress.
                    if frame_started {
                        let mjpeg_complete = frame_buffer.starts_with(&[0xff, 0xd8])
                            && frame_buffer.ends_with(&[0xff, 0xd9]);

                        if (is_eof || mjpeg_complete) && !frame_buffer.is_empty() {
                            println!(
                                "[DIAG] frame #{} complete (eof_bit={}, mjpeg_eoi={}, {} bytes)",
                                frame_count + 1, is_eof, mjpeg_complete, frame_buffer.len()
                            );
                            let complete_frame = std::mem::take(&mut frame_buffer);
                            frame_started = false;
                            emit_frame(&complete_frame, &mut frame_count, actual_frame_size, min_frame_size);

                            if frame_count > captured_before {
                                last_fid = fid;
                                continue 'outer;
                            }
                        }
                    }
                }

                last_fid = fid;
            }
        }

        eprintln!("Warning: no complete frame received after 2000 transfers, try again.");
    }

    // Flush any partially accumulated data left in the buffer at exit.
    // This handles the case where the loop ends mid-frame (e.g. after EXIT).
    if !frame_buffer.is_empty() {
        println!("[DIAG] Flushing partial frame buffer ({} bytes)", frame_buffer.len());
        let complete_frame = std::mem::take(&mut frame_buffer);
        emit_frame(&complete_frame, &mut frame_count, actual_frame_size, min_frame_size);
    }

    println!("\nTotal frames captured: {}", frame_count);

    handle.release_interface(iface_num).ok();
    Ok(())
}