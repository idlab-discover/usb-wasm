// Copyright (c) 2026 IDLab Discover
// SPDX-License-Identifier: MIT

//! YOLOv8 Object Detection Component
//!
//! This component implements a real-time object detection pipeline using the 
//! YOLOv8 model, designed for execution in a WASI-USB sandboxed environment.
//!
//! It leverages the `tract-onnx` crate for performing optimized inference,
//! and `image` for processing raw webcam frames captured via the WASI-USB interface.

#[cfg(target_family = "wasm")]
mod wasm_component {
    use usb_wasm_bindings as bindings;
    use bindings::exports::component::usb::cv::{
        BoundingBox, Detection, Frame, Guest, GuestFrameStream, GuestObjectDetector, Point, Size
    };
    use bindings::exports::wasi::cli::run::Guest as RunGuest;

    use image::{RgbImage, Rgb};
    use ndarray::ArrayViewMutD;
    use tract_onnx::prelude::*;
    use serde::{Deserialize, Serialize};
    use std::fs;
    use std::io::{self, Read};
    use std::time::{Instant, Duration};

    #[derive(Deserialize, Serialize, Clone)]
    struct SerializablePoint {
        x: u32,
        y: u32,
    }

    #[derive(Deserialize, Serialize, Clone)]
    struct SerializableSize {
        width: u32,
        height: u32,
    }

    #[derive(Deserialize, Serialize, Clone)]
    struct SerializableBoundingBox {
        origin: SerializablePoint,
        size: SerializableSize,
    }

    #[derive(Deserialize, Serialize, Clone)]
    struct SerializableDetection {
        label: String,
        confidence: f32,
        #[serde(rename = "box")]
        box_: SerializableBoundingBox,
    }

    /// Draws a rectangle on an RgbImage.
    fn draw_rect(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, color: Rgb<u8>) {
        let width = img.width();
        let height = img.height();

        for i in 0..w {
            let px = x + i;
            if px < width {
                if y < height { img.put_pixel(px, y, color); }
                if y + h.saturating_sub(1) < height { img.put_pixel(px, y + h.saturating_sub(1), color); }
            }
        }
        for i in 0..h {
            let py = y + i;
            if py < height {
                if x < width { img.put_pixel(x, py, color); }
                if x + w.saturating_sub(1) < width { img.put_pixel(x + w.saturating_sub(1), py, color); }
            }
        }
    }

    /// Draws a single character as ASCII Art on an RgbImage.
    fn draw_char(img: &mut RgbImage, x: u32, y: u32, c: char, color: Rgb<u8>, scale: u32) {
        let c = c.to_ascii_uppercase();
        let bitmap: [u8; 5] = match c {
            '0' => [0x3e, 0x51, 0x49, 0x45, 0x3e],
            '1' => [0x00, 0x42, 0x7f, 0x40, 0x00],
            '2' => [0x42, 0x61, 0x51, 0x49, 0x46],
            '3' => [0x21, 0x41, 0x45, 0x4b, 0x31],
            '4' => [0x18, 0x14, 0x12, 0x7f, 0x10],
            '5' => [0x27, 0x45, 0x45, 0x45, 0x39],
            '6' => [0x3c, 0x4a, 0x49, 0x49, 0x30],
            '7' => [0x01, 0x71, 0x09, 0x05, 0x03],
            '8' => [0x36, 0x49, 0x49, 0x49, 0x36],
            '9' => [0x06, 0x49, 0x49, 0x29, 0x1e],
            'A' => [0x7e, 0x11, 0x11, 0x11, 0x7e],
            'B' => [0x7f, 0x49, 0x49, 0x49, 0x36],
            'C' => [0x3e, 0x41, 0x41, 0x41, 0x22],
            'D' => [0x7f, 0x41, 0x41, 0x22, 0x1c],
            'E' => [0x7f, 0x49, 0x49, 0x49, 0x41],
            'F' => [0x7f, 0x09, 0x09, 0x09, 0x01],
            'G' => [0x3e, 0x41, 0x49, 0x49, 0x7a],
            'H' => [0x7f, 0x08, 0x08, 0x08, 0x7f],
            'I' => [0x00, 0x41, 0x7f, 0x41, 0x00],
            'J' => [0x20, 0x40, 0x41, 0x3f, 0x01],
            'K' => [0x7f, 0x08, 0x14, 0x22, 0x41],
            'L' => [0x7f, 0x40, 0x40, 0x40, 0x40],
            'M' => [0x7f, 0x02, 0x0c, 0x02, 0x7f],
            'N' => [0x7f, 0x04, 0x08, 0x10, 0x7f],
            'O' => [0x3e, 0x41, 0x41, 0x41, 0x3e],
            'P' => [0x7f, 0x09, 0x09, 0x09, 0x06],
            'Q' => [0x3e, 0x41, 0x51, 0x21, 0x5e],
            'R' => [0x7f, 0x09, 0x19, 0x29, 0x46],
            'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
            'T' => [0x01, 0x01, 0x7f, 0x01, 0x01],
            'U' => [0x3f, 0x40, 0x40, 0x40, 0x3f],
            'V' => [0x1f, 0x20, 0x40, 0x20, 0x1f],
            'W' => [0x3f, 0x40, 0x38, 0x40, 0x3f],
            'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
            'Y' => [0x07, 0x08, 0x70, 0x08, 0x07],
            'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
            '(' => [0x00, 0x1c, 0x22, 0x41, 0x00],
            ')' => [0x00, 0x41, 0x22, 0x1c, 0x00],
            '.' => [0x00, 0x60, 0x60, 0x00, 0x00],
            ' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
            _ => [0x00, 0x00, 0x00, 0x00, 0x00],
        };

        for i in 0..5 {
            for j in 0..7 {
                if (bitmap[i] >> j) & 1 == 1 {
                    for si in 0..scale {
                        for sj in 0..scale {
                            let px = x + (i as u32 * scale) + si;
                            let py = y + (j as u32 * scale) + sj;
                            if px < img.width() && py < img.height() {
                                img.put_pixel(px, py, color);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Draws a string of text on an RgbImage based on the internal character bitmap.
    fn draw_text(img: &mut RgbImage, x: u32, y: u32, text: &str, color: Rgb<u8>, scale: u32) {
        let mut curr_x = x;
        for c in text.chars() {
            draw_char(img, curr_x, y, c, color, scale);
            curr_x += 6 * scale; // 5 width + 1 spacing, multiplied by scale
        }
    }

    struct YoloDetector;

    impl Guest for YoloDetector {
        type FrameStream = YoloFrameStream;
        type ObjectDetector = YoloObjectDetector;
    }

    impl RunGuest for YoloDetector {
        fn run() -> Result<(), ()> {
            let args = bindings::wasi::cli::environment::get_arguments();
            
            if args.len() >= 3 && args[1] == "--annotate" {
                let json_path = &args[2];
                let camera_index = args.get(3)
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                
                println!("Annotation mode: using JSON {} on camera {}", json_path, camera_index);
                
                let json_content = if json_path == "-" {
                    let mut buffer = String::new();
                    io::stdin().read_to_string(&mut buffer)
                        .map_err(|e| { eprintln!("Failed to read from stdin: {}", e); () })?;
                    buffer
                } else {
                    fs::read_to_string(json_path)
                        .map_err(|e| { eprintln!("Failed to read JSON file: {}", e); () })?
                };

                let detections: Vec<SerializableDetection> = serde_json::from_str(&json_content)
                    .map_err(|e| { eprintln!("Failed to parse JSON: {}", e); () })?;
                
                let stream = bindings::component::usb::cv::FrameStream::new(camera_index);
                println!("Capturing frame...");
                let frame = stream.read_frame()
                    .map_err(|e| { eprintln!("Failed to read frame: {}", e); () })?;
                
                let mut img = RgbImage::from_raw(frame.width, frame.height, frame.data)
                    .ok_or_else(|| { eprintln!("Failed to create image from raw data"); () })?;
                
                for det in &detections {
                    draw_rect(&mut img, det.box_.origin.x, det.box_.origin.y, det.box_.size.width, det.box_.size.height, Rgb([255, 0, 0]));
                    let label = format!("{} ({:.2})", det.label, det.confidence);
                    draw_text(&mut img, det.box_.origin.x, det.box_.origin.y.saturating_sub(16), &label, Rgb([255, 0, 0]), 2);
                }
                
                // Print the detections JSON consistently as a list
                if let Ok(json) = serde_json::to_string(&detections) {
                    println!("{}", json);
                }
                
                img.save("annotated.png")
                    .map_err(|e| { eprintln!("Failed to save image: {}", e); () })?;
                
                println!("Saved annotated image to annotated.png");
                return Ok(());
            }

            let model_path = args.get(1).map(|s| s.as_str()).unwrap_or("yolov8n.onnx");
            let camera_index = args.get(2)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            
            println!("Starting YOLO Demo with model: {} on camera: {}", model_path, camera_index);

            // Use constructor for imported FrameStream (specified camera)
            let stream = bindings::component::usb::cv::FrameStream::new(camera_index);
            
            // Use internal model loading for our own detector
            let model = tract_onnx::onnx()
                .model_for_path(model_path)
                .map_err(|e| { eprintln!("Failed to load model: {}", e); () })?
                .with_input_fact(0, f32::datum_type().fact(&[1, 3, 640, 640]).into())
                .map_err(|e| { eprintln!("Failed to set input fact: {}", e); () })?
                .into_optimized()
                .map_err(|e| { eprintln!("Failed to optimize model: {}", e); () })?
                .into_runnable()
                .map_err(|e| { eprintln!("Failed to make model runnable: {}", e); () })?;

            let detector = YoloObjectDetector { model };

            let mut last_annotated_save = Instant::now();
            let save_interval = Duration::from_secs(5);

            println!("Entering detection loop... (Ctrl+C to stop)");
            loop {
                let frame = stream.read_frame()
                    .map_err(|e| { eprintln!("Failed to read frame: {}", e); () })?;
                
                // Convert imported Frame to exported Frame
                let frame_for_detector = bindings::exports::component::usb::cv::Frame {
                    data: frame.data.clone(),
                    width: frame.width,
                    height: frame.height,
                };
                
                let detections = detector.detect(frame_for_detector)
                    .map_err(|e| { eprintln!("Detection failed: {}", e); () })?;

                if !detections.is_empty() {
                    let serializable: Vec<_> = detections.iter().map(|d| SerializableDetection {
                        label: d.label.clone(),
                        confidence: d.confidence,
                        box_: SerializableBoundingBox {
                            origin: SerializablePoint { x: d.box_.origin.x, y: d.box_.origin.y },
                            size: SerializableSize { width: d.box_.size.width, height: d.box_.size.height },
                        },
                    }).collect();
                    if let Ok(json) = serde_json::to_string(&serializable) {
                        println!("{}", json);
                    }
                }

                if last_annotated_save.elapsed() >= save_interval {
                    if let Some(mut img) = RgbImage::from_raw(frame.width, frame.height, frame.data) {
                        for det in &detections {
                            draw_rect(&mut img, det.box_.origin.x, det.box_.origin.y, det.box_.size.width, det.box_.size.height, Rgb([255, 0, 0]));
                            let label = format!("{} ({:.2})", det.label, det.confidence);
                            draw_text(&mut img, det.box_.origin.x, det.box_.origin.y.saturating_sub(16), &label, Rgb([255, 0, 0]), 2);
                        }
                        if let Ok(_) = img.save("annotated.png") {
                            println!("Periodic annotation saved to annotated.png");
                        }
                    }
                    last_annotated_save = Instant::now();
                }
            }
        }
    }

    struct YoloFrameStream;

    impl GuestFrameStream for YoloFrameStream {
        fn new(_index: u32) -> Self {
            Self
        }

        fn read_frame(&self) -> Result<Frame, String> {
            Err("Not implemented".to_string())
        }
    }

    struct YoloObjectDetector {
        model: SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>,
    }

    impl GuestObjectDetector for YoloObjectDetector {
        fn new(_model_path: String) -> Self {
            panic!("Object detector should be created via main loop for this demo")
        }

        fn detect(&self, f: Frame) -> Result<Vec<Detection>, String> {
        let img = RgbImage::from_raw(f.width, f.height, f.data)
            .ok_or_else(|| "Failed to create image from raw data".to_string())?;

        let resized = image::imageops::resize(&img, 640, 640, image::imageops::FilterType::Triangle);

        let mut input = Tensor::zero::<f32>(&[1, 3, 640, 640]).unwrap();
        {
            let mut input_view: ArrayViewMutD<f32> = input.to_array_view_mut::<f32>().map_err(|e| e.to_string())?;

            for y in 0..640 {
                for x in 0..640 {
                    let pixel = resized.get_pixel(x as u32, y as u32);
                    input_view[[0, 0, y, x]] = pixel[0] as f32 / 255.0;
                    input_view[[0, 1, y, x]] = pixel[1] as f32 / 255.0;
                    input_view[[0, 2, y, x]] = pixel[2] as f32 / 255.0;
                }
            }
        }

        let result = self
            .model
            .run(tvec!(input.into()))
            .map_err(|e| format!("Inference failed: {}", e))?;

        let output = result[0]
            .to_array_view::<f32>()
            .map_err(|e| format!("Failed to get output tensor: {}", e))?;

        // YOLOv8 output shape: [1, 84, 8400]
        let mut detections = Vec::new();
        let num_classes = 80;
        let num_candidates = 8400;

        for i in 0..num_candidates {
            let mut max_score = 0.0;
            let mut class_id = 0;
            for j in 0..num_classes {
                let score = output[[0, 4 + j, i]];
                if score > max_score {
                    max_score = score;
                    class_id = j;
                }
            }

            if max_score > 0.25 {
                let cx = output[[0, 0, i]];
                let cy = output[[0, 1, i]];
                let w = output[[0, 2, i]];
                let h = output[[0, 3, i]];

                let x = (cx - w / 2.0) * f.width as f32;
                let y = (cy - h / 2.0) * f.height as f32;
                let width = w * f.width as f32;
                let height = h * f.height as f32;

                detections.push(Detection {
                    label: COCO_CLASSES[class_id].to_string(),
                    confidence: max_score,
                    box_: BoundingBox {
                        origin: Point {
                            x: x.max(0.0) as u32,
                            y: y.max(0.0) as u32,
                        },
                        size: Size {
                            width: width.max(0.0) as u32,
                            height: height.max(0.0) as u32,
                        },
                    },
                });
            }
        }

        Ok(nms(detections, 0.45))
    }
}

/// Performs Non-Maximum Suppression (NMS) to remove overlapping detections.
fn nms(mut detections: Vec<Detection>, iou_threshold: f32) -> Vec<Detection> {
    detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    let mut kept = Vec::new();
    let mut suppressed = vec![false; detections.len()];

    for i in 0..detections.len() {
        if suppressed[i] {
            continue;
        }
        let det_a = &detections[i];
        kept.push(det_a.clone());
        for j in i + 1..detections.len() {
            if suppressed[j] {
                continue;
            }
            let det_b = &detections[j];
            if intersection_over_union(&det_a.box_, &det_b.box_) > iou_threshold {
                suppressed[j] = true;
            }
        }
    }
    kept
}

/// Calculates the Intersection over Union (IoU) between two bounding boxes.
fn intersection_over_union(box_a: &BoundingBox, box_b: &BoundingBox) -> f32 {
    let x1 = box_a.origin.x.max(box_b.origin.x);
    let y1 = box_a.origin.y.max(box_b.origin.y);
    let x2 = (box_a.origin.x + box_a.size.width).min(box_b.origin.x + box_b.size.width);
    let y2 = (box_a.origin.y + box_a.size.height).min(box_b.origin.y + box_b.size.height);

    let intersection_width = (x2 as f32 - x1 as f32).max(0.0);
    let intersection_height = (y2 as f32 - y1 as f32).max(0.0);
    let intersection_area = intersection_width * intersection_height;

    let area_a = (box_a.size.width * box_a.size.height) as f32;
    let area_b = (box_b.size.width * box_b.size.height) as f32;

    if area_a + area_b - intersection_area <= 0.0 {
        return 0.0;
    }

    intersection_area / (area_a + area_b - intersection_area)
}

const COCO_CLASSES: [&str; 80] = [
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat",
    "traffic light", "fire hydrant", "stop sign", "parking meter", "bench", "bird", "cat",
    "dog", "horse", "sheep", "cow", "elephant", "bear", "zebra", "giraffe", "backpack",
    "umbrella", "handbag", "tie", "suitcase", "frisbee", "skis", "snowboard", "sports ball",
    "kite", "baseball bat", "baseball glove", "skateboard", "surfboard", "tennis racket",
    "bottle", "wine glass", "cup", "fork", "knife", "spoon", "bowl", "banana", "apple",
    "sandwich", "orange", "broccoli", "carrot", "hot dog", "pizza", "donut", "cake",
    "chair", "couch", "potted plant", "bed", "dining table", "toilet", "tv", "laptop",
    "mouse", "remote", "keyboard", "cell phone", "microwave", "oven", "toaster", "sink",
    "refrigerator", "book", "clock", "vase", "scissors", "teddy bear", "hair drier",
    "toothbrush",
];

    bindings::export!(YoloDetector with_types_in bindings);
}
