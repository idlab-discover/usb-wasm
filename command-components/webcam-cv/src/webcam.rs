//! UVC webcam capture via USB isochronous transfers.
//!
//! This module implements a minimal UVC (USB Video Class) capture pipeline:
//! 1. Enumerate USB devices and locate a UVC Video Streaming interface.
//! 2. Select the alternate setting with the highest isochronous bandwidth.
//! 3. Perform the UVC Probe/Commit handshake to negotiate stream parameters.
//!    (Using the camera's default format and resolution to avoid IR streams).
//! 4. Issue isochronous IN transfers and reassemble individual video frames
//!    by tracking UVC payload header flags (FID toggle, EOF bit).
//! 5. Save each complete frame as frame.png and render it as ASCII art.
//!
//! Supported pixel formats: MJPEG and YUYV (YUV 4:2:2 packed).
//!
//! Targets:
//! - wasm32-wasip2 → uses `usb_wasm_bindings` (WASI USB host ABI)
//! - native (macOS) → uses `rusb` + `libusb1-sys` (libusb async isochronous)

use anyhow::{anyhow, Context, Result};
use std::io::{self, BufRead, Write};

// ── Target-specific USB imports ───────────────────────────────────────────────

#[cfg(target_family = "wasm")]
use usb_wasm_bindings::component::usb::{
    device::{list_devices, UsbDevice},
    transfers::{await_iso_transfer, await_transfer, TransferOptions, TransferSetup, TransferType},
};

#[cfg(not(target_family = "wasm"))]
use rusb::UsbContext;
#[cfg(not(target_family = "wasm"))]
use libc;

// ── UVC class constants ───────────────────────────────────────────────────────

const USB_CLASS_VIDEO: u8 = 0x0E;
const USB_SUBCLASS_VIDEO_STREAMING: u8 = 0x02;

// ── ASCII rendering ───────────────────────────────────────────────────────────

const ASCII_CHARS: &[char] = &[' ', '.', ',', '-', '~', ':', ';', '=', '!', '*', '#', '$', '@'];

// ── Interface / endpoint selection (WASM) ────────────────────────────────────

#[cfg(target_family = "wasm")]
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

            let base_size = ep.max_packet_size & 0x7FF;
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

// ── Interface / endpoint selection (native) ───────────────────────────────────

#[cfg(not(target_family = "wasm"))]
fn find_best_streaming_interface_native(
    device: &rusb::Device<rusb::GlobalContext>,
) -> Result<(u8, u8, u8, u16)> {
    let config_desc = device.active_config_descriptor()
        .context("Failed to get active configuration")?;

    let mut best: Option<(u8, u8, u8, u16)> = None;

    for iface in config_desc.interfaces() {
        for iface_desc in iface.descriptors() {
            if iface_desc.class_code() != USB_CLASS_VIDEO
                || iface_desc.sub_class_code() != USB_SUBCLASS_VIDEO_STREAMING
            {
                continue;
            }

            for ep in iface_desc.endpoint_descriptors() {
                use rusb::TransferType;
                let is_iso_in = ep.direction() == rusb::Direction::In
                    && ep.transfer_type() == TransferType::Isochronous;
                if !is_iso_in {
                    continue;
                }

                let mps = ep.max_packet_size();
                let base_size = mps & 0x7FF;
                let multiplier = 1 + ((mps >> 11) & 0x03);
                let effective_size = base_size * multiplier;

                if best.map_or(true, |(_, _, _, s)| effective_size > s) {
                    best = Some((
                        iface_desc.interface_number(),
                        iface_desc.setting_number(),
                        ep.address(),
                        effective_size,
                    ));
                }
            }
        }
    }

    best.ok_or_else(|| anyhow!("No UVC streaming interface found"))
}

// ── UVC payload header parsing ────────────────────────────────────────────────

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
    let idx = (luma as usize * (ASCII_CHARS.len() - 1)) / 255;
    ASCII_CHARS[idx]
}

#[inline]
fn ycbcr_to_rgb_full(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let y  = y  as f32;
    let cb = cb as f32 - 128.0;
    let cr = cr as f32 - 128.0;

    let r = (y + 1.402    * cr                         ).clamp(0.0, 255.0) as u8;
    let g = (y - 0.344136 * cb - 0.714136 * cr         ).clamp(0.0, 255.0) as u8;
    let b = (y + 1.772    * cb                         ).clamp(0.0, 255.0) as u8;
    [r, g, b]
}

#[inline]
fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    ycbcr_to_rgb_full(y, cb, cr)
}

// ── MJPEG decoding ────────────────────────────────────────────────────────────

fn jpeg_component_ids(data: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0xFF && (data[i + 1] == 0xC0 || data[i + 1] == 0xC1) {
            let nf = data[i + 9] as usize;
            let mut ids = Vec::new();
            for c in 0..nf {
                let base = i + 10 + c * 3;
                if base < data.len() {
                    ids.push(data[base]);
                }
            }
            return ids;
        }

        if data[i] == 0xFF && data[i + 1] != 0x00 && data[i + 1] != 0xFF
            && data[i + 1] != 0xD8 && data[i + 1] != 0xD9
        {
            if i + 3 < data.len() {
                let seg_len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
                if seg_len >= 2 {
                    i += 2 + seg_len;
                    continue;
                }
            }
        }
        i += 1;
    }
    vec![]
}

fn inject_colorspace_marker(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return std::borrow::Cow::Borrowed(data);
    }
    if data[2] == 0xFF && (data[3] == 0xE0 || data[3] == 0xEE) {
        return std::borrow::Cow::Borrowed(data);
    }

    let comp_ids = jpeg_component_ids(data);
    let is_rgb_tagged = comp_ids == vec![82u8, 71, 66];

    const ADOBE_APP14_RAW_RGB: &[u8] = &[
        0xFF, 0xEE, 0x00, 0x0E, 0x41, 0x64, 0x6F, 0x62, 0x65, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    const JFIF_APP0: &[u8] = &[
        0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
    ];

    let marker: &[u8] = if is_rgb_tagged { ADOBE_APP14_RAW_RGB } else { JFIF_APP0 };
    let mut out = Vec::with_capacity(data.len() + marker.len());
    out.extend_from_slice(&data[..2]);
    out.extend_from_slice(marker);
    out.extend_from_slice(&data[2..]);
    std::borrow::Cow::Owned(out)
}

fn decode_mjpeg(data: &[u8]) -> Result<image::RgbImage> {
    let _ = std::fs::write("frame_raw.jpg", data);
    let jpeg = inject_colorspace_marker(data);
    let img = image::load_from_memory(jpeg.as_ref())
        .map_err(|e| anyhow::anyhow!("MJPEG decode error: {}", e))?;
    Ok(img.into_rgb8())
}

// ── Frame persistence ─────────────────────────────────────────────────────────

fn save_as_png(frame_data: &[u8], width: u32, height: u32) -> Result<()> {
    use image::{ImageBuffer, Rgb};

    if frame_data.starts_with(&[0xff, 0xd8]) {
        let rgb = decode_mjpeg(frame_data)?;
        rgb.save("frame.png").context("Failed to save frame.png")?;
        println!("Saved frame.png (MJPEG, {}x{})", width, height);
    } else {
        let mut rgb_buf = Vec::with_capacity((width * height * 3) as usize);
        for chunk in frame_data.chunks_exact(4) {
            let y0 = chunk[0];
            let cb = chunk[1];
            let y1 = chunk[2];
            let cr = chunk[3];

            rgb_buf.extend_from_slice(&ycbcr_to_rgb(y0, cb, cr));
            rgb_buf.extend_from_slice(&ycbcr_to_rgb(y1, cb, cr));
        }

        let expected = (width * height * 3) as usize;
        if rgb_buf.len() > expected {
            rgb_buf.truncate(expected);
        } else {
            rgb_buf.resize(expected, 0u8);
        }

        match ImageBuffer::<Rgb<u8>, _>::from_raw(width, height, rgb_buf) {
            Some(buf) => buf.save("frame.png").context("Failed to save frame.png")?,
            None => return Err(anyhow!("Failed to create ImageBuffer ({}x{}, expected {} bytes)", width, height, expected)),
        }
        println!("Saved frame.png (YUYV, {}x{})", width, height);
    }
    Ok(())
}

// ── YUYV resolution detection ─────────────────────────────────────────────────

const KNOWN_RESOLUTIONS: &[(u32, u32)] = &[
    (1920, 1080), (1280, 720), (640, 480), (352, 288), (320, 240), (320, 180),
    (320, 160), (320, 120), (176, 144), (160, 144), (160, 120), (160,  90),
];

fn match_resolution(frame_size: usize) -> Option<(u32, u32)> {
    let mut best: Option<(u32, u32, usize)> = None;
    for &(w, h) in KNOWN_RESOLUTIONS {
        let expected = (w * h * 2) as usize;
        let diff = frame_size.abs_diff(expected);
        if diff * 20 <= expected && best.map_or(true, |(_, _, d)| diff < d) {
            best = Some((w, h, diff));
        }
    }
    best.map(|(w, h, _)| (w, h))
}

fn get_resolution(frame_size: usize, _negotiated_size: u32) -> (u32, u32) {
    match_resolution(frame_size).unwrap_or_else(|| {
        if frame_size >= 153_600      { (320, 240) }
        else if frame_size >= 102_400 { (320, 160) }
        else if frame_size >=  76_800 { (320, 120) }
        else if frame_size >=  50_688 { (176, 144) }
        else if frame_size >=  46_080 { (160, 144) }
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
                let byte_idx = (y * width + x) as usize * 2;
                if byte_idx < frame_data.len() {
                    let macropixel_idx = (y * width + (x & !1)) as usize * 2;
                    let y_offset = ((x & 1) * 2) as usize;
                    let luma = frame_data[macropixel_idx + y_offset];
                    let cb = frame_data[macropixel_idx + 1];
                    let cr = frame_data[macropixel_idx + 3];
                    let [r, g, b] = ycbcr_to_rgb(luma, cb, cr);
                    ascii.push(rgb_to_ascii(r, g, b));
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
    frame_count:    &mut u32,
    actual_frame_size: u32,
    min_frame_size: usize,
) {
    if complete_frame.len() < min_frame_size {
        eprintln!("Skipping fragment ({} bytes < minimum {} bytes)", complete_frame.len(), min_frame_size);
        return;
    }

    let is_mjpeg = complete_frame.starts_with(&[0xff, 0xd8]);
    if !is_mjpeg && match_resolution(complete_frame.len()).is_none() {
        eprintln!("Skipping YUYV frame with unrecognized size ({} bytes)", complete_frame.len());
        return;
    }

    *frame_count += 1;

    let (w, h) = if is_mjpeg {
        match decode_mjpeg(complete_frame) {
            Ok(img)  => (img.width(), img.height()),
            Err(e)   => { eprintln!("JPEG decode error: {:?}", e); return; }
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
        Err(e) => eprintln!("Frame render error: {:?}", e),
    }
}

// ── UVC frame reassembly (shared logic) ──────────────────────────────────────

fn process_iso_buffer(
    flat_data:      &[u8],
    packet_lengths: &[usize], 
    packet_stride:  usize,
    frame_buffer:   &mut Vec<u8>,
    frame_count:    &mut u32,
    frame_started:  &mut bool,
    last_fid:       &mut u8,
    actual_frame_size: u32,
    min_frame_size: usize,
    captured_before: u32,
) -> bool {
    let mut offset = 0usize;
    let mut emitted = false;

    for actual_len in packet_lengths {
        let actual_len = *actual_len;
        if actual_len == 0 {
            offset += packet_stride;
            continue;
        }

        let pkt_data = &flat_data[offset..offset + actual_len];
        offset += packet_stride;

        let (hdr_len, is_eof) = parse_payload_header(pkt_data);
        if hdr_len == 0 { continue; }

        let fid     = pkt_data[1] & 0x01;
        let payload = if hdr_len < pkt_data.len() { &pkt_data[hdr_len..] } else { &[][..] };

        let fid_toggle = !frame_buffer.is_empty() && fid != *last_fid;

        if fid_toggle {
            if *frame_started { frame_buffer.clear(); } else { frame_buffer.clear(); }
            
            if payload.starts_with(&[0xff, 0xd8]) {
                frame_buffer.extend_from_slice(payload);
                *frame_started = true;
            } else {
                *frame_started = true;
                frame_buffer.extend_from_slice(payload);
            }

            if *frame_count > captured_before {
                emitted = true;
                *last_fid = fid;
                return emitted;
            }
        } else {
            if !*frame_started {
                if let Some(soi_pos) = payload.windows(2).position(|w| w == [0xff, 0xd8]) {
                    frame_buffer.extend_from_slice(&payload[soi_pos..]);
                    *frame_started = true;
                } else if !payload.is_empty() {
                    frame_buffer.extend_from_slice(payload);
                    *frame_started = true;
                }
            } else {
                frame_buffer.extend_from_slice(payload);
            }

            if *frame_started && is_eof && !frame_buffer.is_empty() {
                let complete_frame = std::mem::take(frame_buffer);
                *frame_started = false;
                emit_frame(&complete_frame, frame_count, actual_frame_size, min_frame_size);

                if *frame_count > captured_before {
                    emitted = true;
                    *last_fid = fid;
                    return emitted;
                }
            }
        }
        *last_fid = fid;
    }
    emitted
}

// ── Main entry point (WASM) ───────────────────────────────────────────────────

#[cfg(target_family = "wasm")]
pub fn run_webcam() -> Result<()> {
    println!("Starting UVC webcam capture via isochronous transfers (WASM)...");

    let devices = list_devices().map_err(|e| anyhow!("{:?}", e))?;
    let (device, descriptor, location) = devices
        .into_iter()
        .find(|(_, desc, _)| desc.device_class == USB_CLASS_VIDEO || desc.device_class == 0xEF)
        .ok_or_else(|| anyhow!("No UVC device found (class 0x0E or 0xEF)"))?;

    println!("Found UVC device: {:04x}:{:04x} at bus {} address {}", descriptor.vendor_id, descriptor.product_id, location.bus_number, location.device_address);

    let handle = device.open().map_err(|e| anyhow!("{:?}", e)).context("Failed to open device")?;
    let (iface_num, alt_setting, ep_addr, max_packet_size) = find_best_streaming_interface(&device)?;

    handle.claim_interface(iface_num).map_err(|e| anyhow!("{:?}", e)).context("Failed to claim interface")?;

    // STAP 1: Alt Setting moet STRICT 0 (idle) zijn tijdens de handshake
    handle.set_interface_altsetting(iface_num, 0).ok();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // STAP 2: Bepaal correcte UVC 1.1 configuratie (PROBE -> GET -> SET)
    let final_f_idx = 2; // MJPEG
    let final_fr_idx = 1; // Default Frame (bvb 1080p of 720p)
    let mut actual_frame_size = 0;
    
    println!("Performing strict UVC Probe/Commit handshake...");

    // Lees basis PROBE (GET_CUR)
    let probe_get = handle.new_transfer(
        TransferType::Control, TransferSetup { bm_request_type: 0xA1, b_request: 0x81, w_value: 0x0100, w_index: iface_num as u16 },
        34, TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
    ).map_err(|e| anyhow!("{:?}", e))?;
    probe_get.submit_transfer(&[]).ok();
    
    let mut baseline_probe = await_transfer(probe_get).unwrap_or(vec![0; 26]);
    if baseline_probe.len() >= 4 {
        baseline_probe[2] = final_f_idx;
        baseline_probe[3] = final_fr_idx;
    }

    // Stuur gewenste configuratie door (SET_CUR PROBE)
    let probe_set = handle.new_transfer(
        TransferType::Control, TransferSetup { bm_request_type: 0x21, b_request: 0x01, w_value: 0x0100, w_index: iface_num as u16 },
        baseline_probe.len() as u32, TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
    ).map_err(|e| anyhow!("{:?}", e))?;
    probe_set.submit_transfer(&baseline_probe).ok();
    await_transfer(probe_set).ok();

    // Vraag op wat de camera beslist heeft! (GET_CUR PROBE) - ESSENTIEEL
    let probe_get_again = handle.new_transfer(
        TransferType::Control, TransferSetup { bm_request_type: 0xA1, b_request: 0x81, w_value: 0x0100, w_index: iface_num as u16 },
        baseline_probe.len() as u32, TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
    ).map_err(|e| anyhow!("{:?}", e))?;
    probe_get_again.submit_transfer(&[]).ok();
    let negotiated_probe = await_transfer(probe_get_again).unwrap_or(baseline_probe.clone());

    actual_frame_size = if negotiated_probe.len() >= 22 {
        u32::from_le_bytes(negotiated_probe[18..22].try_into().unwrap())
    } else { 0 };

    // Bevestig definitief de EXACTE output van de camera (SET_CUR COMMIT)
    let commit_set = handle.new_transfer(
        TransferType::Control, TransferSetup { bm_request_type: 0x21, b_request: 0x01, w_value: 0x0200, w_index: iface_num as u16 },
        negotiated_probe.len() as u32, TransferOptions { endpoint: 0, timeout_ms: 2000, stream_id: 0, iso_packets: 0 },
    ).map_err(|e| anyhow!("{:?}", e))?;
    commit_set.submit_transfer(&negotiated_probe).ok();
    await_transfer(commit_set).ok();

    println!("Handshake complete. Validated maxSize={} bytes", actual_frame_size);

    // STAP 3: Stel Auto White Balance / Kleuren in NÁ de handshake
    println!("Initializing Processing Unit (AWB and ISP)...");
    let vc_iface: u16 = 0; 
    let pu_unit: u16 = 2;  
    
    // AWB
    let awb_xfer = handle.new_transfer(
        TransferType::Control, TransferSetup { bm_request_type: 0x21, b_request: 0x01, w_value: 0x0B00, w_index: (pu_unit << 8) | vc_iface },
        1, TransferOptions { endpoint: 0, timeout_ms: 1000, stream_id: 0, iso_packets: 0 },
    );
    if let Ok(xfer) = awb_xfer {
        if xfer.submit_transfer(&[1]).is_ok() { 
            let _ = await_transfer(xfer);
            println!("  PU_WHITE_BALANCE_TEMPERATURE_AUTO = 1 OK");
        }
    }

    // STAP 4: Activeer de Interface pas NU op de juiste bandbreedte
    println!("Activating interface stream (Alt Setting {})...", alt_setting);
    handle.set_interface_altsetting(iface_num, alt_setting).ok();

    // ── Isochronous transfer parameters ──
    let num_packets:  u32 = 32;
    let packet_stride: u32 = max_packet_size as u32;
    let buffer_size:  u32 = num_packets * packet_stride;

    let opts = TransferOptions {
        endpoint: ep_addr, timeout_ms: 2000, stream_id: 0, iso_packets: num_packets,
    };

    let min_frame_size: usize = 28_800;
    let mut frame_buffer:  Vec<u8> = Vec::new();
    let mut frame_count:   u32 = 0;
    let mut last_fid:      u8  = 0;
    let mut frame_started: bool = false;

    println!("Camera ISP warm-up (2 seconds)...");
    std::thread::sleep(std::time::Duration::from_secs(2));

    let stdin = io::stdin();
    'outer: loop {
        print!("Press ENTER for frame #{} (or EXIT): ", frame_count + 1);
        io::stdout().flush().ok();
        let mut input = String::new();
        if stdin.lock().read_line(&mut input).is_err() || input.trim().eq_ignore_ascii_case("exit") { break; }

        println!("Capturing...");
        frame_buffer.clear();
        frame_started = false;
        let captured_before = frame_count;

        for _attempt in 0..2000 {
            let xfer = handle.new_transfer(
                TransferType::Isochronous, TransferSetup { bm_request_type: 0, b_request: 0, w_value: 0, w_index: 0 },
                buffer_size, opts.clone(),
            ).map_err(|e| anyhow!("{:?}", e))?;

            xfer.submit_transfer(&[]).map_err(|e| anyhow!("{:?}", e))?;
            let iso_result = await_iso_transfer(xfer).map_err(|e| anyhow!("{:?}", e))?;
            let lengths: Vec<usize> = iso_result.packets.iter().map(|p| p.actual_length as usize).collect();

            if process_iso_buffer(
                &iso_result.data, &lengths, packet_stride as usize, &mut frame_buffer, &mut frame_count,
                &mut frame_started, &mut last_fid, actual_frame_size, min_frame_size, captured_before,
            ) { continue 'outer; }
        }

        if !frame_buffer.is_empty() {
            let complete_frame = std::mem::take(&mut frame_buffer);
            emit_frame(&complete_frame, &mut frame_count, actual_frame_size, min_frame_size);
        }
    }
    handle.release_interface(iface_num).ok();
    Ok(())
}

// ── Main entry point (native) ─────────────────────────────────────────────────

#[cfg(not(target_family = "wasm"))]
pub fn run_webcam() -> Result<()> {
    use std::time::Duration;
    use libusb1_sys::*;

    println!("Starting UVC webcam capture (native / libusb)...");

    let ctx = rusb::GlobalContext::default();
    let devices = ctx.devices().context("Failed to list USB devices")?;
    let mut found_device: Option<rusb::Device<rusb::GlobalContext>> = None;
    for device in devices.iter() {
        let desc = match device.device_descriptor() { Ok(d) => d, Err(_) => continue };
        if desc.class_code() == USB_CLASS_VIDEO || desc.class_code() == 0xEF {
            found_device = Some(device);
            break;
        }
    }
    let device = found_device.ok_or_else(|| anyhow!("No UVC device found"))?;
    let (iface_num, alt_setting, ep_addr, max_packet_size) = find_best_streaming_interface_native(&device)?;

    let handle = device.open().context("Failed to open device")?;
    handle.set_auto_detach_kernel_driver(true).ok();
    handle.claim_interface(iface_num).context("Failed to claim interface")?;

    // STAP 1: Idle
    handle.set_alternate_setting(iface_num, 0).ok();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // STAP 2: Handshake
    println!("Performing strict UVC Probe/Commit handshake...");
    let timeout2s = Duration::from_millis(2000);
    
    let mut baseline_probe = vec![0u8; 34];
    let n = handle.read_control(0xA1, 0x81, 0x0100, iface_num as u16, &mut baseline_probe, timeout2s).unwrap_or(26);
    baseline_probe.truncate(n);
    
    if baseline_probe.len() >= 4 {
        baseline_probe[2] = 2; // MJPEG
        baseline_probe[3] = 1; // Default frame
    }

    handle.write_control(0x21, 0x01, 0x0100, iface_num as u16, &baseline_probe, timeout2s).ok(); // SET PROBE
    
    let mut negotiated_probe = vec![0u8; baseline_probe.len()];
    handle.read_control(0xA1, 0x81, 0x0100, iface_num as u16, &mut negotiated_probe, timeout2s).ok(); // GET PROBE
    
    handle.write_control(0x21, 0x01, 0x0200, iface_num as u16, &negotiated_probe, timeout2s).ok(); // SET COMMIT

    let actual_frame_size = if negotiated_probe.len() >= 22 {
        u32::from_le_bytes(negotiated_probe[18..22].try_into().unwrap())
    } else { 0 };
    println!("Handshake complete. Validated maxSize={} bytes", actual_frame_size);

    // STAP 3: AWB
    println!("Initializing Processing Unit (AWB and ISP)...");
    let vc_iface: u16 = 0; 
    let pu_unit: u16 = 2;  
    if handle.write_control(0x21, 0x01, 0x0B00, (pu_unit << 8) | vc_iface, &[1], timeout2s).is_ok() {
        println!("  PU_WHITE_BALANCE_TEMPERATURE_AUTO = 1 OK");
    }

    // STAP 4: Activeer stream
    println!("Activating interface stream (Alt Setting {})...", alt_setting);
    handle.set_alternate_setting(iface_num, alt_setting).context("Failed to set alt setting")?;

    let num_packets: u32 = 32;
    let packet_stride: u32 = max_packet_size as u32;
    let buffer_size: u32 = num_packets * packet_stride;
    let min_frame_size: usize = 28_800;
    let mut frame_buffer:  Vec<u8> = Vec::new();
    let mut frame_count:   u32 = 0;
    let mut last_fid:      u8 = 0;
    let mut frame_started: bool = false;

    println!("Camera ISP warm-up (2 seconds)...");
    std::thread::sleep(std::time::Duration::from_secs(2));

    let stdin = io::stdin();
    let raw_handle = handle.as_raw();

    'outer: loop {
        print!("Press ENTER for frame #{} (or EXIT): ", frame_count + 1);
        io::stdout().flush().ok();
        let mut input = String::new();
        if stdin.lock().read_line(&mut input).is_err() || input.trim().eq_ignore_ascii_case("exit") { break; }

        frame_buffer.clear();
        frame_started = false;
        last_fid = 0;
        let captured_before = frame_count;

        for _attempt in 0..2000 {
            let xfer = unsafe { libusb_alloc_transfer(num_packets as i32) };
            if xfer.is_null() { return Err(anyhow!("libusb_alloc_transfer returned NULL")); }
            let mut buf = vec![0u8; buffer_size as usize];
            let completed = Box::new(std::sync::atomic::AtomicBool::new(false));
            let completed_ptr = &*completed as *const std::sync::atomic::AtomicBool;

            extern "system" fn iso_callback(transfer: *mut libusb1_sys::libusb_transfer) {
                unsafe { (*( (*transfer).user_data as *const std::sync::atomic::AtomicBool )).store(true, std::sync::atomic::Ordering::Release); }
            }

            unsafe {
                libusb1_sys::libusb_fill_iso_transfer(
                    xfer, raw_handle, ep_addr, buf.as_mut_ptr(), buffer_size as i32, num_packets as i32,
                    iso_callback, completed_ptr as *mut libc::c_void, 2000, 
                );
                libusb1_sys::libusb_set_iso_packet_lengths(xfer, packet_stride);
            }

            if unsafe { libusb1_sys::libusb_submit_transfer(xfer) } != 0 {
                unsafe { libusb_free_transfer(xfer) };
                return Err(anyhow!("libusb_submit_transfer failed"));
            }

            let libusb_ctx = std::ptr::null_mut::<libusb1_sys::libusb_context>();
            let deadline = std::time::Instant::now() + Duration::from_millis(2500);
            loop {
                if completed.load(std::sync::atomic::Ordering::Acquire) { break; }
                if std::time::Instant::now() > deadline { unsafe { libusb1_sys::libusb_cancel_transfer(xfer) }; break; }
                let mut tv = libc::timeval { tv_sec: 0, tv_usec: 10_000 };
                unsafe { libusb1_sys::libusb_handle_events_timeout(libusb_ctx, &mut tv) };
            }

            let mut lengths = Vec::with_capacity(num_packets as usize);
            for i in 0..num_packets as usize {
                lengths.push(unsafe { (*(*xfer).iso_packet_desc.as_ptr().add(i)).actual_length as usize });
            }
            unsafe { libusb_free_transfer(xfer) };

            if process_iso_buffer(
                &buf, &lengths, packet_stride as usize, &mut frame_buffer, &mut frame_count,
                &mut frame_started, &mut last_fid, actual_frame_size, min_frame_size, captured_before,
            ) { continue 'outer; }
        }
    }
    handle.release_interface(iface_num).ok();
    Ok(())
}