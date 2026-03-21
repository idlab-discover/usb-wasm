#[cfg(target_family = "wasm")]
mod wasm_component {
    use usb_wasm_bindings as bindings;
    use bindings::exports::component::usb::cv::{
        BoundingBox, Detection, Frame, Guest, GuestFrameStream, GuestObjectDetector, Point, Size,
        FrameStream, ObjectDetector
    };
    use bindings::exports::wasi::cli::run::Guest as RunGuest;

    use image::RgbImage;
    use ndarray::ArrayViewMutD;
    use tract_onnx::prelude::*;

    struct YoloDetector;

    impl Guest for YoloDetector {
        type FrameStream = YoloFrameStream;
        type ObjectDetector = YoloObjectDetector;
    }

    impl RunGuest for YoloDetector {
        fn run() -> Result<(), ()> {
            let args = bindings::wasi::cli::environment::get_arguments();
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

            println!("Entering detection loop... (Ctrl+C to stop)");
            loop {
                let frame = stream.read_frame()
                    .map_err(|e| { eprintln!("Failed to read frame: {}", e); () })?;
                
                // Convert imported Frame to exported Frame
                let frame_for_detector = bindings::exports::component::usb::cv::Frame {
                    data: frame.data,
                    width: frame.width,
                    height: frame.height,
                };
                
                let detections = detector.detect(frame_for_detector)
                    .map_err(|e| { eprintln!("Detection failed: {}", e); () })?;

                if !detections.is_empty() {
                    println!("Detections: {:?}", detections);
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

                let x = (cx - w / 2.0) * (f.width as f32 / 640.0);
                let y = (cy - h / 2.0) * (f.height as f32 / 640.0);
                let width = w * (f.width as f32 / 640.0);
                let height = h * (f.height as f32 / 640.0);

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
