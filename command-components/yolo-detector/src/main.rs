// Copyright (c) 2026 IDLab Discover
// SPDX-License-Identifier: MIT

//! YOLOv8 Detector Command
//!
//! This component performs real-time object detection using YOLOv8 via 
//! WASI-USB camera stream. It runs inference entirely in WebAssembly!

use anyhow::{Result, anyhow};
use usb_wasm_bindings::frame_transport::{FrameSource, RawFrame};
use tract_onnx::prelude::*;
use image::RgbImage;


#[derive(Debug, Clone)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub origin: Point,
    pub size: Size,
}

#[derive(Debug, Clone)]
pub struct Detection {
    pub label: String,
    pub confidence: f32,
    pub box_: BoundingBox,
}



pub fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let model_path = args.get(1).map(|s| s.as_str()).unwrap_or("yolov8n.onnx");
    let out_dir   = args.get(2).map(|s| s.as_str()).unwrap_or(".");

    println!("Initializing YOLOv8 detector in WASM (model: {})...", model_path);

    // 1. Load the model purely in WASM
    let model = tract_onnx::onnx()
        .model_for_path(model_path)
        .map_err(|e| anyhow!("Failed to load model from {}: {:?}", model_path, e))?
        .with_input_fact(0, f32::datum_type().fact(&[1, 3, 640, 640]).into())?
        .into_optimized()?
        .into_runnable()?;

    let camera_index = 0;
    println!("Initializing FrameSource for camera #{}...", camera_index);
    let stream = FrameSource::new(camera_index);

    // Discard the first batch of frames so the camera ISP / AWB has time to converge.
    // 300 frames ≈ 10 s at 30 fps — same warm-up time the old interactive app gave
    // implicitly while the user waited before pressing ENTER.
    println!("ISP warm-up: discarding first 300 frames (~10 s)...");
    for i in 0..300u32 {
        let _ = stream.next_frame();
        if i % 50 == 49 { println!("  warm-up {}/300", i + 1); }
    }
    println!("ISP warm-up complete, starting detection loop.");

    println!("Detection loop started. Saving annotated frames to '{}'.", out_dir);
    let mut frame_idx = 0u32;
    loop {
        match stream.next_frame() {
            Ok(frame) => {
                // ── DIAGNOSTIC: save raw frame at idx 0 and idx 20 ───────────
                // Lets us see whether the camera ISP settles after a few frames.
                if frame_idx == 0 || frame_idx == 20 {
                    let tag = if frame_idx == 0 { "early" } else { "later" };
                    let _ = std::fs::write(format!("{}/dbg_raw_{}.jpg", out_dir, tag), &frame.data);
                    eprintln!("DIAG frame {}: wrote dbg_raw_{}.jpg", frame_idx, tag);
                }
                // ─────────────────────────────────────────────────────────────

                let start = std::time::Instant::now();
                match decode_and_detect(&model, &frame) {
                    Ok((mut img, detections)) => {
                        let duration = start.elapsed();

                        // Annotate: draw bounding boxes + labels on the decoded image
                        for det in &detections {
                            let b = &det.box_;
                            let color = image::Rgb([0u8, 255u8, 0u8]);
                            draw_rect(
                                &mut img,
                                b.origin.x, b.origin.y,
                                b.size.width, b.size.height,
                                color,
                            );
                            // Label: "classname XX%" just above the top-left corner (font: 10×14 @ scale 2)
                            let label = format!("{} {:.0}%", det.label, det.confidence * 100.0);
                            let label_y = b.origin.y.saturating_sub(16);
                            draw_text(&mut img, &label, b.origin.x + 2, label_y, color);
                        }

                        // Save annotated frame as JPEG
                        let path = format!("{}/frame_{:05}.jpg", out_dir, frame_idx);
                        if let Err(e) = img.save(&path) {
                            eprintln!("Save error {}: {}", path, e);
                        }
                        frame_idx += 1;

                        // Console summary
                        print!("\x1B[2J\x1B[H");
                        println!("Frame {:05} — {} objects (inference: {:?}):", frame_idx, detections.len(), duration);
                        for det in &detections {
                            println!(
                                "  [{:.0}%] {} @ ({},{}) {}×{}",
                                det.confidence * 100.0,
                                det.label,
                                det.box_.origin.x, det.box_.origin.y,
                                det.box_.size.width, det.box_.size.height,
                            );
                        }
                        println!("  → saved {}", path);
                    }
                    Err(e) => eprintln!("Detection error: {}", e),
                }
            }
            Err(e) => eprintln!("Capture error: {}", e),
        }
    }
}



/// UVC MJPEG streams may lack a JFIF APP0 header entirely. Without it,
/// `jpeg-decoder` may not perform YCbCr→RGB conversion, producing a pink tint.
/// If the JPEG already has any APP0 (FF E0) or APP14 (FF EE) marker, we leave
/// it completely alone — `jpeg-decoder` handles AVI1/Motion-JPEG fine via
/// component-ID-based colorspace detection. We only inject a minimal JFIF APP0
/// when there is no such marker at all.
/// For the special case where component IDs spell "RGB" ([82,71,66]) we inject
/// an Adobe APP14 with color_transform=0 so the decoder treats data as raw RGB.
fn inject_colorspace_marker(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return std::borrow::Cow::Borrowed(data);
    }
    // Already has APP0 or APP14 → leave untouched (handles AVI1, JFIF, Adobe)
    if data[2] == 0xFF && (data[3] == 0xE0 || data[3] == 0xEE) {
        return std::borrow::Cow::Borrowed(data);
    }

    let comp_ids = jpeg_component_ids(data);
    let is_rgb_tagged = comp_ids == [82u8, 71, 66];

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

/// Walk the JPEG segment list to collect SOF component IDs.
fn jpeg_component_ids(data: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0xFF && (data[i + 1] == 0xC0 || data[i + 1] == 0xC1) {
            let nf = data[i + 9] as usize;
            let mut ids = Vec::new();
            for c in 0..nf {
                let base = i + 10 + c * 3;
                if base < data.len() { ids.push(data[base]); }
            }
            return ids;
        }
        if data[i] == 0xFF && data[i + 1] != 0x00 && data[i + 1] != 0xFF
            && data[i + 1] != 0xD8 && data[i + 1] != 0xD9
        {
            if i + 3 < data.len() {
                let seg_len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
                if seg_len >= 2 { i += 2 + seg_len; continue; }
            }
        }
        i += 1;
    }
    vec![]
}

/// Decode the raw frame, run YOLO inference, and return the decoded RGB image
/// alongside the detections so the caller can annotate and save it.
fn decode_and_detect(
    model: &SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>,
    f: &RawFrame,
) -> Result<(RgbImage, Vec<Detection>)> {
    // Decode: MJPEG (inject JFIF only when no APP0/APP14 present) or YUYV fallback
    let img = if f.data.starts_with(&[0xFF, 0xD8]) {
        let jpeg = inject_colorspace_marker(&f.data);
        image::load_from_memory(jpeg.as_ref())
            .map_err(|e| anyhow!("MJPEG decode: {}", e))?
            .to_rgb8()
    } else if let Ok(dynamic) = image::load_from_memory(&f.data) {
        dynamic.to_rgb8()
    } else {
        yuyv_to_rgb(&f.data, f.width, f.height)?
    };

    let resized = image::imageops::resize(&img, 640, 640, image::imageops::FilterType::Triangle);

    let mut input = Tensor::from_shape(&[1, 3, 640, 640], &[0.0f32; 1 * 3 * 640 * 640])?;
    {
        let mut view = input.to_array_view_mut::<f32>()?;
        for y in 0..640usize {
            for x in 0..640usize {
                let p = resized.get_pixel(x as u32, y as u32);
                view[[0, 0, y, x]] = p[0] as f32 / 255.0;
                view[[0, 1, y, x]] = p[1] as f32 / 255.0;
                view[[0, 2, y, x]] = p[2] as f32 / 255.0;
            }
        }
    }

    // 3. Inference
    let result = model.run(tvec!(input.into()))?;
    let output = result[0].to_array_view::<f32>()?;

    // 4. Postprocessing: Tensor -> Detections -> NMS
    let mut detections = Vec::new();
    for i in 0..8400 {
        let mut max_score = 0.0f32;
        let mut class_id = 0usize;
        for j in 0..80 {
            let score = output[[0, 4 + j, i]];
            if score > max_score { max_score = score; class_id = j; }
        }
        if max_score > 0.25 {
            let cx = output[[0, 0, i]];
            let cy = output[[0, 1, i]];
            let bw = output[[0, 2, i]];
            let bh = output[[0, 3, i]];
            // YOLO outputs normalised coords (0-1) relative to the 640×640
            // input. Scale back to the *decoded* image dimensions, not the
            // RawFrame's estimated width/height (which uses the YUYV formula
            // and is wrong for MJPEG frames).
            let iw = img.width() as f32;
            let ih = img.height() as f32;
            let x = ((cx - bw / 2.0) * iw).max(0.0) as u32;
            let y = ((cy - bh / 2.0) * ih).max(0.0) as u32;
            detections.push(Detection {
                label: COCO_CLASSES[class_id].to_string(),
                confidence: max_score,
                box_: BoundingBox {
                    origin: Point { x, y },
                    size: Size {
                        width: (bw * iw) as u32,
                        height: (bh * ih) as u32,
                    },
                },
            });
        }
    }

    Ok((img, nms(detections, 0.45)))
}

/// Render ASCII text onto an RGB image using a 5×7 pixel bitmap font.
/// `scale` stretches each pixel into a `scale×scale` block (e.g. 2 → 10×14 per char).
fn draw_text(img: &mut RgbImage, text: &str, mut x: u32, y: u32, color: image::Rgb<u8>) {
    draw_text_scaled(img, text, x, y, color, 2);
}

fn draw_text_scaled(img: &mut RgbImage, text: &str, mut x: u32, y: u32, color: image::Rgb<u8>, scale: u32) {
    let (iw, ih) = img.dimensions();
    for ch in text.chars() {
        let bitmap = char_bitmap(ch);
        for row in 0..7u32 {
            for col in 0..5u32 {
                if bitmap[row as usize] & (1 << (4 - col)) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = x + col * scale + dx;
                            let py = y + row * scale + dy;
                            if px < iw && py < ih {
                                img.put_pixel(px, py, color);
                            }
                        }
                    }
                }
            }
        }
        x += (5 + 1) * scale; // (glyph width + gap) × scale
        if x >= iw { break; }
    }
}

/// 5×7 bitmap font — each entry is 7 rows of 5 bits (MSB = leftmost pixel).
fn char_bitmap(c: char) -> [u8; 7] {
    match c {
        'A' => [0x0E,0x11,0x11,0x1F,0x11,0x11,0x11],
        'B' => [0x1E,0x11,0x11,0x1E,0x11,0x11,0x1E],
        'C' => [0x0E,0x11,0x10,0x10,0x10,0x11,0x0E],
        'D' => [0x1E,0x09,0x09,0x09,0x09,0x09,0x1E],
        'E' => [0x1F,0x10,0x10,0x1E,0x10,0x10,0x1F],
        'F' => [0x1F,0x10,0x10,0x1E,0x10,0x10,0x10],
        'G' => [0x0E,0x11,0x10,0x17,0x11,0x11,0x0F],
        'H' => [0x11,0x11,0x11,0x1F,0x11,0x11,0x11],
        'I' => [0x0E,0x04,0x04,0x04,0x04,0x04,0x0E],
        'J' => [0x07,0x02,0x02,0x02,0x02,0x12,0x0C],
        'K' => [0x11,0x12,0x14,0x18,0x14,0x12,0x11],
        'L' => [0x10,0x10,0x10,0x10,0x10,0x10,0x1F],
        'M' => [0x11,0x1B,0x15,0x11,0x11,0x11,0x11],
        'N' => [0x11,0x19,0x15,0x13,0x11,0x11,0x11],
        'O' => [0x0E,0x11,0x11,0x11,0x11,0x11,0x0E],
        'P' => [0x1E,0x11,0x11,0x1E,0x10,0x10,0x10],
        'Q' => [0x0E,0x11,0x11,0x11,0x15,0x12,0x0D],
        'R' => [0x1E,0x11,0x11,0x1E,0x14,0x12,0x11],
        'S' => [0x0F,0x10,0x10,0x0E,0x01,0x01,0x1E],
        'T' => [0x1F,0x04,0x04,0x04,0x04,0x04,0x04],
        'U' => [0x11,0x11,0x11,0x11,0x11,0x11,0x0E],
        'V' => [0x11,0x11,0x11,0x11,0x11,0x0A,0x04],
        'W' => [0x11,0x11,0x11,0x15,0x15,0x1B,0x11],
        'X' => [0x11,0x11,0x0A,0x04,0x0A,0x11,0x11],
        'Y' => [0x11,0x11,0x0A,0x04,0x04,0x04,0x04],
        'Z' => [0x1F,0x01,0x02,0x04,0x08,0x10,0x1F],
        'a' => [0x00,0x00,0x0E,0x01,0x0F,0x11,0x0F],
        'b' => [0x10,0x10,0x1C,0x12,0x12,0x12,0x1C],
        'c' => [0x00,0x00,0x0E,0x10,0x10,0x11,0x0E],
        'd' => [0x01,0x01,0x07,0x09,0x09,0x09,0x07],
        'e' => [0x00,0x00,0x0E,0x11,0x1F,0x10,0x0F],
        'f' => [0x06,0x09,0x08,0x1C,0x08,0x08,0x08],
        'g' => [0x00,0x00,0x0F,0x11,0x0F,0x01,0x0E],
        'h' => [0x10,0x10,0x1C,0x12,0x12,0x12,0x12],
        'i' => [0x04,0x00,0x0C,0x04,0x04,0x04,0x0E],
        'j' => [0x02,0x00,0x06,0x02,0x02,0x12,0x0C],
        'k' => [0x10,0x10,0x12,0x14,0x18,0x14,0x12],
        'l' => [0x0C,0x04,0x04,0x04,0x04,0x04,0x0E],
        'm' => [0x00,0x00,0x1A,0x15,0x15,0x11,0x11],
        'n' => [0x00,0x00,0x1C,0x12,0x12,0x12,0x12],
        'o' => [0x00,0x00,0x0E,0x11,0x11,0x11,0x0E],
        'p' => [0x00,0x00,0x1C,0x12,0x1C,0x10,0x10],
        'q' => [0x00,0x00,0x07,0x09,0x07,0x01,0x01],
        'r' => [0x00,0x00,0x16,0x18,0x10,0x10,0x10],
        's' => [0x00,0x00,0x0E,0x10,0x0C,0x01,0x1E],
        't' => [0x08,0x08,0x1C,0x08,0x08,0x09,0x06],
        'u' => [0x00,0x00,0x12,0x12,0x12,0x12,0x0D],
        'v' => [0x00,0x00,0x11,0x11,0x11,0x0A,0x04],
        'w' => [0x00,0x00,0x11,0x11,0x15,0x15,0x0A],
        'x' => [0x00,0x00,0x11,0x0A,0x04,0x0A,0x11],
        'y' => [0x00,0x00,0x11,0x11,0x0F,0x01,0x0E],
        'z' => [0x00,0x00,0x1F,0x02,0x04,0x08,0x1F],
        '0' => [0x0E,0x11,0x13,0x15,0x19,0x11,0x0E],
        '1' => [0x04,0x0C,0x04,0x04,0x04,0x04,0x0E],
        '2' => [0x0E,0x11,0x01,0x06,0x08,0x10,0x1F],
        '3' => [0x1F,0x02,0x04,0x06,0x01,0x11,0x0E],
        '4' => [0x02,0x06,0x0A,0x12,0x1F,0x02,0x02],
        '5' => [0x1F,0x10,0x1E,0x01,0x01,0x11,0x0E],
        '6' => [0x06,0x08,0x10,0x1E,0x11,0x11,0x0E],
        '7' => [0x1F,0x01,0x02,0x04,0x08,0x08,0x08],
        '8' => [0x0E,0x11,0x11,0x0E,0x11,0x11,0x0E],
        '9' => [0x0E,0x11,0x11,0x0F,0x01,0x02,0x0C],
        '%' => [0x18,0x19,0x02,0x04,0x08,0x13,0x03],
        '.' => [0x00,0x00,0x00,0x00,0x00,0x00,0x04],
        ':' => [0x00,0x04,0x00,0x00,0x00,0x04,0x00],
        ' ' => [0x00,0x00,0x00,0x00,0x00,0x00,0x00],
        _   => [0x15,0x0A,0x15,0x0A,0x15,0x0A,0x15], // checkerboard for unknown
    }
}

/// Draw a 2-pixel-thick rectangle outline onto an RGB image.
fn draw_rect(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, color: image::Rgb<u8>) {
    let (iw, ih) = img.dimensions();
    let x2 = (x + w).min(iw.saturating_sub(1));
    let y2 = (y + h).min(ih.saturating_sub(1));

    for t in 0u32..2 {
        // Top / bottom horizontal edges
        for px in x..=x2 {
            if y + t < ih     { img.put_pixel(px, y + t,          color); }
            if y2 >= t        { img.put_pixel(px, y2 - t,         color); }
        }
        // Left / right vertical edges
        for py in y..=y2 {
            if x + t < iw     { img.put_pixel(x + t,  py,         color); }
            if x2 >= t        { img.put_pixel(x2 - t, py,         color); }
        }
    }
}

fn yuyv_to_rgb(data: &[u8], width: u32, height: u32) -> Result<RgbImage> {
    if data.len() < (width * height * 2) as usize {
        return Err(anyhow!("Data too small for YUYV"));
    }
    let mut rgb = RgbImage::new(width, height);
    for i in 0..(width * height / 2) {
        let y0 = data[i as usize * 4] as f32;
        let u = data[i as usize * 4 + 1] as f32 - 128.0;
        let y1 = data[i as usize * 4 + 2] as f32;
        let v = data[i as usize * 4 + 3] as f32 - 128.0;

        let r0 = (y0 + 1.402 * v).clamp(0.0, 255.0) as u8;
        let g0 = (y0 - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8;
        let b0 = (y0 + 1.772 * u).clamp(0.0, 255.0) as u8;

        let r1 = (y1 + 1.402 * v).clamp(0.0, 255.0) as u8;
        let g1 = (y1 - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8;
        let b1 = (y1 + 1.772 * u).clamp(0.0, 255.0) as u8;

        rgb.put_pixel((i * 2) % width, (i * 2) / width, image::Rgb([r0, g0, b0]));
        rgb.put_pixel((i * 2 + 1) % width, (i * 2 + 1) / width, image::Rgb([r1, g1, b1]));
    }
    Ok(rgb)
}

fn nms(mut detections: Vec<Detection>, iou_threshold: f32) -> Vec<Detection> {
    detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    let mut kept = Vec::new();
    let mut suppressed = vec![false; detections.len()];
    for i in 0..detections.len() {
        if suppressed[i] { continue; }
        kept.push(detections[i].clone());
        for j in (i + 1)..detections.len() {
            if suppressed[j] { continue; }
            if iou(&detections[i].box_, &detections[j].box_) > iou_threshold {
                suppressed[j] = true;
            }
        }
    }
    kept
}

fn iou(a: &BoundingBox, b: &BoundingBox) -> f32 {
    let x1 = a.origin.x.max(b.origin.x);
    let y1 = a.origin.y.max(b.origin.y);
    let x2 = (a.origin.x + a.size.width).min(b.origin.x + b.size.width);
    let y2 = (a.origin.y + a.size.height).min(b.origin.y + b.size.height);
    let inter = (x2 as f32 - x1 as f32).max(0.0) * (y2 as f32 - y1 as f32).max(0.0);
    let area_a = (a.size.width * a.size.height) as f32;
    let area_b = (b.size.width * b.size.height) as f32;
    inter / (area_a + area_b - inter)
}

const COCO_CLASSES: [&str; 80] = [
    "person","bicycle","car","motorcycle","airplane","bus","train","truck","boat",
    "traffic light","fire hydrant","stop sign","parking meter","bench","bird","cat",
    "dog","horse","sheep","cow","elephant","bear","zebra","giraffe","backpack",
    "umbrella","handbag","tie","suitcase","frisbee","skis","snowboard","sports ball",
    "kite","baseball bat","baseball glove","skateboard","surfboard","tennis racket",
    "bottle","wine glass","cup","fork","knife","spoon","bowl","banana","apple",
    "sandwich","orange","broccoli","carrot","hot dog","pizza","donut","cake",
    "chair","couch","potted plant","bed","dining table","toilet","tv","laptop",
    "mouse","remote","keyboard","cell phone","microwave","oven","toaster","sink",
    "refrigerator","book","clock","vase","scissors","teddy bear","hair drier","toothbrush",
];

