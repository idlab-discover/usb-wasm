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

use anyhow::{anyhow, Context, Result};
use std::io::{self, BufRead, Write};

use usb_wasm_bindings::component::usb::{
    device::{list_devices, UsbDevice},
    transfers::{await_iso_transfer, await_transfer, TransferOptions, TransferSetup, TransferType},
};

// ── UVC class constants ───────────────────────────────────────────────────────

const USB_CLASS_VIDEO: u8 = 0x0E;
const USB_SUBCLASS_VIDEO_STREAMING: u8 = 0x02;

// ── ASCII rendering ───────────────────────────────────────────────────────────

const ASCII_CHARS: &[char] = &[' ', '.', ',', '-', '~', ':', ';', '=', '!', '*', '#', '$', '@'];

// ── Interface / endpoint selection ───────────────────────────────────────────

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
            let is_iso_in = (ep.endpoint_address & 0x80 != 0) && (ep.attributes & 0x03 == 1);
            if !is_iso_in {
                continue;
            }

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

/// Returns `(header_length_in_bytes, end_of_frame_flag)`.
fn parse_payload_header(data: &[u8]) -> (usize, bool) {
    if data.len() < 2 {
        return (0, false);
    }
    let header_len = data[0] as usize;
    if header_len < 2 || header_len > data.len() {
        return (0, false);
    }
    let end_of_frame = (data[1] & 0x02) != 0;
    (header_len, end_of_frame)
}

// ── Pixel conversion helpers ──────────────────────────────────────────────────

fn rgb_to_ascii(r: u8, g: u8, b: u8) -> char {
    let luma = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8;
    let idx  = (luma as usize * (ASCII_CHARS.len() - 1)) / 255;
    ASCII_CHARS[idx]
}

// ── BT.601 YCbCr → RGB conversion (shared by MJPEG fallback and YUYV) ────────

/// Convert a single YCbCr sample (full-range BT.601) to RGB.
#[inline]
fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let y  = y  as f32;
    let cb = cb as f32 - 128.0;
    let cr = cr as f32 - 128.0;

    let r = (y                  + 1.402   * cr).clamp(0.0, 255.0) as u8;
    let g = (y - 0.344136 * cb - 0.714136 * cr).clamp(0.0, 255.0) as u8;
    let b = (y + 1.772   * cb              ).clamp(0.0, 255.0) as u8;
    [r, g, b]
}

// ── MJPEG decoding ────────────────────────────────────────────────────────────

/// Decode an MJPEG frame to a full-color RgbImage.
///
/// Many UVC cameras emit MJPEG without a JFIF APP0 or Adobe APP14 marker, which
/// causes the underlying `jpeg-decoder` crate to be uncertain about the colorspace.
/// Depending on the component IDs embedded in the SOF segment, it may:
///   a) Correctly detect YCbCr and convert → proper colors.
///   b) Treat the three components as raw RGB → channel-shifted colors.
///   c) Return only the Y (luma) plane → monochrome output.
///
/// Strategy:
///   1. Decode with `image::load_from_memory`. If the result is already a proper
///      color image (chrominance variance > threshold), return it as-is.
///   2. If the decoded image looks monochrome (all channels equal), the decoder
///      likely returned the raw Y plane only. Scan the raw JPEG bytes for the SOF0
///      segment to find the actual number of components. If there are 3 components,
///      decode again using the `jpeg_decoder` crate directly with an explicit
///      YCbCr pixel-format request, then convert manually.
///   3. If the JPEG truly only has 1 component, it is genuinely grayscale.
///
/// No gray-world or other post-process color correction is applied here: those
/// corrections can easily turn a low-saturation scene into a monochrome one.
/// Inject an Adobe APP14 marker with ColorTransform=0 after the SOI.
///
/// The Logitech Brio (and similar cameras) encode the raw sensor RGB data
/// directly into the JPEG DCT coefficients, but label the components with
/// IDs [1,2,3] (the standard YCbCr labeling). This causes conforming
/// decoders (including jpeg-decoder and macOS Preview) to apply a spurious
/// YCbCr->RGB conversion on data that is already RGB, producing a strong
/// magenta/pink cast.
///
/// Adobe APP14 with ColorTransform=0 ("unknown / no transform") instructs
/// jpeg-decoder to skip the colorspace conversion and return the raw
/// component values, which for this camera are already correct sRGB.
///
/// If a colorspace marker is already present (JFIF APP0 or Adobe APP14)
/// the data is returned unchanged to avoid conflicting signals.
fn force_rgb_colorspace(data: &[u8]) -> std::borrow::Cow<[u8]> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return std::borrow::Cow::Borrowed(data);
    }
    // If APP0 (JFIF) or APP14 (Adobe) already present, leave it alone.
    if data[2] == 0xFF && (data[3] == 0xE0 || data[3] == 0xEE) {
        return std::borrow::Cow::Borrowed(data);
    }
    // Adobe APP14 segment, ColorTransform = 0 (raw RGB, no YCbCr conversion):
    //   FF EE          APP14 marker
    //   00 0E          segment length = 14 (includes the 2 length bytes)
    //   41 64 6F 62 65 "Adobe"
    //   00 64          DCTEncodeVersion = 100
    //   00 00          Flags0
    //   00 00          Flags1
    //   00             ColorTransform: 0 = no color transform (raw RGB)
    const ADOBE_APP14: &[u8] = &[
        0xFF, 0xEE, 0x00, 0x0E,
        0x41, 0x64, 0x6F, 0x62, 0x65,
        0x00, 0x64,
        0x00, 0x00,
        0x00, 0x00,
        0x00,
    ];
    let mut out = Vec::with_capacity(data.len() + ADOBE_APP14.len());
    out.extend_from_slice(&data[..2]); // SOI
    out.extend_from_slice(ADOBE_APP14);
    out.extend_from_slice(&data[2..]); // rest of JPEG
    std::borrow::Cow::Owned(out)
}

fn jpeg_component_ids(data: &[u8]) -> Vec<u8> {
    // Scan for SOF0 (FF C0) or SOF1 (FF C1) and return the component IDs.
    // Component IDs tell us whether this is YCbCr (IDs 1,2,3) or RGB (IDs 82,71,66 = R,G,B).
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0xFF && (data[i+1] == 0xC0 || data[i+1] == 0xC1) {
            // SOF layout: FF Cn LL LL PP HH HH WW WW Nf [Ci Hv Tq]...
            //              0  1  2  3  4  5  6  7  8  9   10 11 12
            let nf = data[i+9] as usize;
            let mut ids = Vec::new();
            for c in 0..nf {
                let base = i + 10 + c * 3;
                if base < data.len() {
                    ids.push(data[base]);
                }
            }
            return ids;
        }
        // Skip segment: read length field
        if data[i] == 0xFF && data[i+1] != 0x00 && data[i+1] != 0xFF
            && data[i+1] != 0xD8 && data[i+1] != 0xD9
        {
            if i + 3 < data.len() {
                let seg_len = u16::from_be_bytes([data[i+2], data[i+3]]) as usize;
                if seg_len >= 2 { i += 2 + seg_len; continue; }
            }
        }
        i += 1;
    }
    vec![]
}

fn decode_mjpeg(data: &[u8]) -> Result<image::RgbImage> {
    // Save the raw JPEG bytes so the user can open frame_raw.jpg directly
    // in Preview / a browser to verify whether the issue is camera-side or decode-side.
    let _ = std::fs::write("frame_raw.jpg", data);

    // Log component IDs from the JPEG SOF segment.
    // IDs 1,2,3 → standard YCbCr (jpeg_decoder WILL convert).
    // IDs 82,71,66 → ASCII 'R','G','B' → decoder treats as raw RGB (no conversion).
    // Other IDs → non-standard; behavior depends on decoder heuristics.
    let comp_ids = jpeg_component_ids(data);
    eprintln!("[JPEG DIAG] SOF component IDs: {:?}", comp_ids);
    let is_standard_ycbcr = comp_ids == vec![1u8, 2, 3];
    let is_rgb_tagged     = comp_ids == vec![82u8, 71, 66]; // 'R','G','B'
    eprintln!("[JPEG DIAG] standard_ycbcr={} rgb_tagged={}", is_standard_ycbcr, is_rgb_tagged);

    // Inject JFIF APP0 if missing so the decoder knows the stream is YCbCr.
    // Without this marker jpeg-decoder passes raw YCbCr bytes through as RGB,
    // producing a magenta cast (Y->R, Cb->G, Cr->B misinterpretation).
    let jpeg = force_rgb_colorspace(data);

    // Log first raw pixels from jpeg_decoder for diagnostics.
    {
        use jpeg_decoder::Decoder;
        use std::io::Cursor;
        let mut dbg = Decoder::new(Cursor::new(jpeg.as_ref()));
        if let Ok(px) = dbg.decode() {
            let info = dbg.info();
            eprintln!(
                "[COLOR DIAG] fmt={:?} first12={:?}",
                info.map(|i| format!("{:?}", i.pixel_format)).unwrap_or_default(),
                &px[..px.len().min(12)]
            );
        }
    }

    let img = image::load_from_memory(jpeg.as_ref())
        .context("MJPEG decode failed")?;
    Ok(img.into_rgb8())
}


// ── Frame persistence ─────────────────────────────────────────────────────────

/// Save a captured frame to "frame.png".
///
/// - MJPEG: detected by the 0xFF 0xD8 SOI marker; decoded via `decode_mjpeg`.
/// - YUYV:  YUV 4:2:2 packed; each 4-byte macro-pixel encodes two pixels.
fn save_as_png(frame_data: &[u8], width: u32, height: u32) -> Result<()> {
    use image::{ImageBuffer, Rgb};

    if frame_data.starts_with(&[0xff, 0xd8]) {
        let rgb = decode_mjpeg(frame_data)?;
        rgb.save("frame.png").context("Failed to save frame.png")?;
        println!("Saved frame.png (MJPEG, {}x{})", width, height);
    } else {
        // YUYV 4:2:2: bytes are [Y0, Cb, Y1, Cr] per macro-pixel.
        let mut rgb_buf = Vec::with_capacity((width * height * 3) as usize);

        for chunk in frame_data.chunks_exact(4) {
            let y0 = chunk[0];
            let cb = chunk[1];
            let y1 = chunk[2];
            let cr = chunk[3];

            rgb_buf.extend_from_slice(&ycbcr_to_rgb(y0, cb, cr));
            rgb_buf.extend_from_slice(&ycbcr_to_rgb(y1, cb, cr));
        }

        // Pad or truncate to exact expected size.
        let expected = (width * height * 3) as usize;
        if rgb_buf.len() > expected {
            rgb_buf.truncate(expected);
        } else {
            rgb_buf.resize(expected, 0u8);
        }

        match ImageBuffer::<Rgb<u8>, _>::from_raw(width, height, rgb_buf) {
            Some(buf) => buf.save("frame.png").context("Failed to save frame.png")?,
            None => return Err(anyhow!(
                "Failed to create ImageBuffer ({}x{}, expected {} bytes)",
                width, height, expected
            )),
        }
        println!("Saved frame.png (YUYV, {}x{})", width, height);
    }

    Ok(())
}

// ── YUYV resolution detection ─────────────────────────────────────────────────

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

fn match_resolution(frame_size: usize) -> Option<(u32, u32)> {
    let mut best: Option<(u32, u32, usize)> = None;

    for &(w, h) in KNOWN_RESOLUTIONS {
        let expected = (w * h * 2) as usize;
        let diff     = frame_size.abs_diff(expected);

        if diff * 20 <= expected && best.map_or(true, |(_, _, d)| diff < d) {
            best = Some((w, h, diff));
        }
    }

    best.map(|(w, h, _)| (w, h))
}

fn get_resolution(frame_size: usize, _negotiated_size: u32) -> (u32, u32) {
    match_resolution(frame_size).unwrap_or_else(|| {
        if      frame_size >= 153_600 { (320, 240) }
        else if frame_size >= 102_400 { (320, 160) }
        else if frame_size >= 76_800  { (320, 120) }
        else if frame_size >= 50_688  { (176, 144) }
        else if frame_size >= 46_080  { (160, 144) }
        else                          { (160, 120) }
    })
}

// ── ASCII rendering ───────────────────────────────────────────────────────────

fn process_frame(frame_data: &[u8], negotiated_size: u32) -> Result<String> {
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
    let target_height = (height as f32 * (target_width as f32 / width as f32) * 0.5) as u32;

    let x_step = width  as f32 / target_width  as f32;
    let y_step = height as f32 / target_height as f32;

    let mut ascii = String::with_capacity(((target_width + 1) * target_height) as usize);

    for ty in 0..target_height {
        for tx in 0..target_width {
            let x = (tx as f32 * x_step) as u32;
            let y = (ty as f32 * y_step) as u32;

            if let Some(ref img) = maybe_img {
                let pixel = img.get_pixel(x.min(width - 1), y.min(height - 1));
                let image::Rgb([r, g, b]) = *pixel;
                ascii.push(rgb_to_ascii(r, g, b));
            } else {
                // YUYV: luma byte for pixel (x, y) is at offset (y * width + x) * 2.
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

    if !is_mjpeg && match_resolution(complete_frame.len()).is_none() {
        eprintln!(
            "Skipping YUYV frame with unrecognized size ({} bytes) -- likely incomplete",
            complete_frame.len()
        );
        return;
    }

    *frame_count += 1;

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

    let devices = list_devices().map_err(|e| anyhow!("{:?}", e))?;
    println!("Found {} USB device(s)", devices.len());

    let (device, descriptor, location) = devices
        .into_iter()
        .find(|(_, desc, _)| desc.device_class == USB_CLASS_VIDEO || desc.device_class == 0xEF)
        .ok_or_else(|| anyhow!("No UVC device found (class 0x0E or 0xEF)"))?;

    println!(
        "Found UVC device: {:04x}:{:04x} at bus {} address {}",
        descriptor.vendor_id, descriptor.product_id,
        location.bus_number,  location.device_address
    );

    let handle: usb_wasm_bindings::component::usb::device::DeviceHandle = device
        .open()
        .map_err(|e| anyhow!("{:?}", e))
        .context("Failed to open device")?;

    let (iface_num, alt_setting, ep_addr, max_packet_size) =
        find_best_streaming_interface(&device)?;

    println!(
        "Selected interface {}, alt setting {}, endpoint 0x{:02x}, max packet size {} bytes",
        iface_num, alt_setting, ep_addr, max_packet_size
    );

    handle
        .claim_interface(iface_num)
        .map_err(|e| anyhow!("{:?}", e))
        .context("Failed to claim interface")?;

    // ── UVC Processing Unit initialization ───────────────────────────────────
    // Many cameras (including Logitech) start with auto white balance disabled,
    // producing a magenta cast. Send SET_CUR to enable AWB and auto hue via
    // the Processing Unit (unit ID 2, Video Control interface 0).
    // We check GET_INFO first so unsupported controls are silently skipped.
    {
        let vc_iface: u16 = 0;
        let pu_unit:  u16 = 2;
        let pu_controls: &[(u8, u16, &str)] = &[
            (1, 0x0B, "PU_WHITE_BALANCE_TEMPERATURE_AUTO"),
            (1, 0x0D, "PU_WHITE_BALANCE_COMPONENT_AUTO"),
            (1, 0x10, "PU_HUE_AUTO"),
        ];
        println!("Initializing Processing Unit (auto white balance / hue)...");
        for &(value, cs, name) in pu_controls {
            let w_value = cs << 8;
            let w_index = (pu_unit << 8) | vc_iface;
            let info_ok = handle.new_transfer(
                TransferType::Control,
                TransferSetup { bm_request_type: 0xA1, b_request: 0x86, w_value, w_index },
                1,
                TransferOptions { endpoint: 0, timeout_ms: 500, stream_id: 0, iso_packets: 0 },
            ).ok().and_then(|xfer| {
                xfer.submit_transfer(&[]).ok()?;
                let info = await_transfer(xfer).ok()?;
                info.first().copied()
            });
            match info_ok {
                Some(caps) if caps & 0x04 != 0 => {
                    if let Ok(xfer) = handle.new_transfer(
                        TransferType::Control,
                        TransferSetup { bm_request_type: 0x21, b_request: 0x01, w_value, w_index },
                        1,
                        TransferOptions { endpoint: 0, timeout_ms: 500, stream_id: 0, iso_packets: 0 },
                    ) {
                        let _ = xfer.submit_transfer(&[value]);
                        match await_transfer(xfer) {
                            Ok(_)  => println!("  {} = {} OK", name, value),
                            Err(e) => println!("  {} failed: {:?}", name, e),
                        }
                    }
                }
                Some(caps) => println!("  {} not writable (caps=0x{:02x})", name, caps),
                None       => println!("  {} not supported", name),
            }
        }
    }

    println!("Resetting interface to alt setting 0...");
    handle.set_interface_altsetting(iface_num, 0).ok();
    handle
        .set_interface_altsetting(iface_num, alt_setting)
        .map_err(|e| anyhow!("{:?}", e))
        .context("Failed to set alt setting")?;
    println!("Set alt setting {}", alt_setting);

    // ── UVC Probe/Commit handshake ────────────────────────────────────────────

    println!("Performing UVC Probe/Commit handshake...");

    let probe_xfer = handle.new_transfer(
        TransferType::Control,
        TransferSetup {
            bm_request_type: 0xA1,
            b_request: 0x81,     // GET_CUR
            w_value:   0x0100,   // VS_PROBE_CONTROL
            w_index:   iface_num as u16,
        },
        26,
        TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
    ).map_err(|e| anyhow!("{:?}", e))?;

    probe_xfer.submit_transfer(&[]).map_err(|e| anyhow!("{:?}", e))?;
    let probe_data = await_transfer(probe_xfer).map_err(|e| anyhow!("{:?}", e))?;
    println!("  Probe GET_CUR: {} bytes received", probe_data.len());

    let mut actual_frame_size = 0u32;

    if probe_data.len() >= 26 {
        actual_frame_size = u32::from_le_bytes(probe_data[18..22].try_into().unwrap());
        let cur_format = probe_data[2];
        let cur_frame  = probe_data[3];
        println!(
            "  Default format index: {}, frame index: {}, frame interval: {} (100 ns units)",
            cur_format, cur_frame,
            u32::from_le_bytes(probe_data[4..8].try_into().unwrap())
        );
        println!("  dwMaxVideoFrameSize: {} bytes", actual_frame_size);

        // ── Query MIN and MAX to enumerate all supported formats ─────────────
        // GET_MIN (0x82) and GET_MAX (0x83) on VS_PROBE_CONTROL report the
        // range of format/frame indices the camera supports. Logging these
        // helps identify which format index corresponds to MJPEG vs YUYV.
        for (req_code, req_name) in [(0x82u8, "GET_MIN"), (0x83u8, "GET_MAX")] {
            if let Ok(xfer) = handle.new_transfer(
                TransferType::Control,
                TransferSetup {
                    bm_request_type: 0xA1,
                    b_request: req_code,
                    w_value:   0x0100,
                    w_index:   iface_num as u16,
                },
                26,
                TransferOptions { endpoint: 0, timeout_ms: 1000, stream_id: 0, iso_packets: 0 },
            ) {
                let _ = xfer.submit_transfer(&[]);
                if let Ok(d) = await_transfer(xfer) {
                    if d.len() >= 4 {
                        println!("  {}: format={} frame={}", req_name, d[2], d[3]);
                    }
                }
            }
        }

        // ── Try each format index 1..=4 with GET_CUR to find MJPEG ──────────
        // We send a probe SET_CUR with bFormatIndex=N, then GET_CUR to see
        // what the camera accepted and what frame size it reports.
        // We pick the format+frame combo with the largest dwMaxVideoFrameSize
        // (MJPEG frames are much larger than YUYV at the same resolution).
        let mut best_format_idx = cur_format;
        let mut best_frame_idx  = cur_frame;
        let mut best_frame_size = actual_frame_size;

        for fmt_idx in 1u8..=4 {
            let mut probe_try = probe_data.clone();
            probe_try[2] = fmt_idx; // bFormatIndex
            probe_try[3] = 1;       // bFrameIndex = 1 (first/only frame for this format)

            // SET_CUR probe with candidate format
            if let Ok(xfer) = handle.new_transfer(
                TransferType::Control,
                TransferSetup {
                    bm_request_type: 0x21,
                    b_request: 0x01,
                    w_value:   0x0100,
                    w_index:   iface_num as u16,
                },
                probe_try.len() as u32,
                TransferOptions { endpoint: 0, timeout_ms: 1000, stream_id: 0, iso_packets: 0 },
            ) {
                let _ = xfer.submit_transfer(&probe_try);
                let _ = await_transfer(xfer);
            }

            // GET_CUR to see what the camera actually accepted
            if let Ok(xfer) = handle.new_transfer(
                TransferType::Control,
                TransferSetup {
                    bm_request_type: 0xA1,
                    b_request: 0x81,
                    w_value:   0x0100,
                    w_index:   iface_num as u16,
                },
                26,
                TransferOptions { endpoint: 0, timeout_ms: 1000, stream_id: 0, iso_packets: 0 },
            ) {
                let _ = xfer.submit_transfer(&[]);
                if let Ok(d) = await_transfer(xfer) {
                    if d.len() >= 22 {
                        let sz = u32::from_le_bytes(d[18..22].try_into().unwrap());
                        let accepted_fmt = d[2];
                        let accepted_frm = d[3];
                        println!(
                            "  Format probe {}: camera accepted fmt={} frame={} maxSize={} bytes",
                            fmt_idx, accepted_fmt, accepted_frm, sz
                        );
                        // Prefer larger frame size == MJPEG (compressed, varies)
                        // YUYV for 640x480 is exactly 614400 bytes;
                        // MJPEG for 640x480 is typically 40000-200000 bytes.
                        // We want the *smallest non-zero* dwMaxVideoFrameSize that
                        // is still plausibly MJPEG (< 400000), as that indicates
                        // actual variable-length compression.
                        if sz > 0 && sz < 400_000 && sz > best_frame_size {
                            best_frame_size  = sz;
                            best_format_idx  = accepted_fmt;
                            best_frame_idx   = accepted_frm;
                        }
                    }
                }
            }
        }

        println!(
            "  Selected: format={} frame={} maxSize={} bytes",
            best_format_idx, best_frame_idx, best_frame_size
        );
        actual_frame_size = best_frame_size;

        // ── Final Probe/Commit with chosen format ────────────────────────────
        let mut final_probe = probe_data.clone();
        final_probe[2] = best_format_idx;
        final_probe[3] = best_frame_idx;

        let set_probe = handle.new_transfer(
            TransferType::Control,
            TransferSetup {
                bm_request_type: 0x21,
                b_request: 0x01,   // SET_CUR
                w_value:   0x0100, // VS_PROBE_CONTROL
                w_index:   iface_num as u16,
            },
            final_probe.len() as u32,
            TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
        ).map_err(|e| anyhow!("{:?}", e))?;
        set_probe.submit_transfer(&final_probe).map_err(|e| anyhow!("{:?}", e))?;
        await_transfer(set_probe).map_err(|e| anyhow!("{:?}", e))?;

        let commit = handle.new_transfer(
            TransferType::Control,
            TransferSetup {
                bm_request_type: 0x21,
                b_request: 0x01,   // SET_CUR
                w_value:   0x0200, // VS_COMMIT_CONTROL
                w_index:   iface_num as u16,
            },
            final_probe.len() as u32,
            TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
        ).map_err(|e| anyhow!("{:?}", e))?;
        commit.submit_transfer(&final_probe).map_err(|e| anyhow!("{:?}", e))?;
        await_transfer(commit).map_err(|e| anyhow!("{:?}", e))?;

        println!("Handshake complete");
    }

    // ── Isochronous transfer parameters ──────────────────────────────────────

    let num_packets:   u32 = 32;
    let packet_stride: u32 = max_packet_size as u32;
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

    let min_frame_size: usize = 28_800;
    println!("Minimum frame size: {} bytes", min_frame_size);

    // ── Interactive capture loop ──────────────────────────────────────────────

    let mut frame_buffer:  Vec<u8> = Vec::new();
    let mut frame_count:   u32     = 0;
    let mut last_fid:      u8      = 0;
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
        frame_started = false;
        let captured_before = frame_count;

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
                    offset += packet_stride as usize;
                    continue;
                }

                let pkt_data = &flat_data[offset..offset + actual_len];
                offset += packet_stride as usize;

                let (hdr_len, is_eof) = parse_payload_header(pkt_data);
                if hdr_len == 0 {
                    continue;
                }

                if frame_count == captured_before && i < 2 {
                    println!(
                        "[DIAG] packet {}: HLE=0x{:02x} BFH=0b{:08b} length={}",
                        i, pkt_data[0], pkt_data[1], actual_len
                    );
                }

                let fid     = pkt_data[1] & 0x01;
                let payload = if hdr_len < pkt_data.len() { &pkt_data[hdr_len..] } else { &[][..] };

                let fid_toggle = !frame_buffer.is_empty() && fid != last_fid;

                if fid_toggle {
                    if frame_started {
                        println!(
                            "[DIAG] FID toggle -> emitting frame #{} ({} bytes)",
                            frame_count + 1, frame_buffer.len()
                        );
                        let complete_frame = std::mem::take(&mut frame_buffer);
                        emit_frame(&complete_frame, &mut frame_count, actual_frame_size, min_frame_size);
                    } else {
                        println!("[DIAG] FID toggle, no SOI seen yet — discarding {} bytes", frame_buffer.len());
                        frame_buffer.clear();
                    }

                    if payload.starts_with(&[0xff, 0xd8]) {
                        frame_buffer.extend_from_slice(payload);
                        frame_started = true;
                    } else {
                        frame_started = false;
                        println!(
                            "[DIAG] FID toggle payload does not start with SOI \
                             (0x{:02x} 0x{:02x}) — waiting",
                            payload.first().copied().unwrap_or(0),
                            payload.get(1).copied().unwrap_or(0)
                        );
                    }

                    if frame_count > captured_before {
                        last_fid = fid;
                        continue 'outer;
                    }
                } else {
                    if !frame_started {
                        if let Some(soi_pos) = payload.windows(2).position(|w| w == [0xff, 0xd8]) {
                            frame_buffer.extend_from_slice(&payload[soi_pos..]);
                            frame_started = true;
                            println!("[DIAG] SOI found at offset {} in payload — accumulation started", soi_pos);
                        }
                    } else {
                        frame_buffer.extend_from_slice(payload);
                    }

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

    if !frame_buffer.is_empty() {
        println!("[DIAG] Flushing partial frame buffer ({} bytes)", frame_buffer.len());
        let complete_frame = std::mem::take(&mut frame_buffer);
        emit_frame(&complete_frame, &mut frame_count, actual_frame_size, min_frame_size);
    }

    println!("\nTotal frames captured: {}", frame_count);

    handle.release_interface(iface_num).ok();
    Ok(())
}