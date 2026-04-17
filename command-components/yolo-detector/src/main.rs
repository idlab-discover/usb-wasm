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

    println!("Detection loop started. Press Ctrl+C to stop.");
    loop {
        match stream.next_frame() {
            Ok(frame) => {
                let start = std::time::Instant::now();
                match process_and_detect(&model, &frame) {


                    Ok(detections) => {
                        let duration = start.elapsed();
                        // Clear screen (ANSI)
                        print!("\x1B[2J\x1B[H");
                        println!("Detected {} objects (Inference time: {:?}):", detections.len(), duration);
                        for det in &detections {
                            println!(
                                "- {}: {:.1}% at ({}, {})",
                                det.label,
                                det.confidence * 100.0,
                                det.box_.origin.x,
                                det.box_.origin.y,
                            );
                        }
                    }
                    Err(e) => eprintln!("Detection error: {}", e),
                }
            }
            Err(e) => eprintln!("Capture error: {}", e),
        }
    }
}



fn process_and_detect(model: &SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>, f: &RawFrame) -> Result<Vec<Detection>> {

    // 2. Preprocessing: Frame -> RgbImage -> Resized -> Tensor
    // Handle MJPEG or YUYV
    let img = if let Ok(dynamic) = image::load_from_memory(&f.data) {
        dynamic.to_rgb8()
    } else {
        // Fallback or assume YUYV if not JPEG
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
            let x = ((cx - bw / 2.0) * f.width as f32).max(0.0) as u32;
            let y = ((cy - bh / 2.0) * f.height as f32).max(0.0) as u32;
            detections.push(Detection {
                label: COCO_CLASSES[class_id].to_string(),
                confidence: max_score,
                box_: BoundingBox {
                    origin: Point { x, y },
                    size: Size {
                        width: (bw * f.width as f32) as u32,
                        height: (bh * f.height as f32) as u32,
                    },
                },
            });
        }
    }

    Ok(nms(detections, 0.45))
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

