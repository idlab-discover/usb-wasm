use crate::bindings::component::usb::device::{Host as HostDevice, HostUsbDevice, HostDeviceHandle, DeviceLocation};
use crate::bindings::component::usb::descriptors::{DeviceDescriptor, ConfigurationDescriptor};
use wasmtime_wasi::IoView;
use crate::bindings::component::usb::configuration::ConfigValue;
use crate::bindings::component::usb::transfers::{Host as HostTransfers, HostTransfer, IsoPacket, IsoPacketStatus, TransferResult, TransferType, TransferSetup, TransferOptions};
use crate::bindings::component::usb::cv::{HostFrameStream, HostObjectDetector, Detection, Frame, Point, Size, BoundingBox};
use crate::bindings::component::usb::errors::LibusbError;
use crate::bindings::component::usb::usb_hotplug::{Host as HostHotplug, Event, Info};
use crate::{UsbDevice, UsbDeviceHandle, UsbTransfer, ObjectDetector, FrameStream, UsbView};
use wasmtime::component::Resource;

use std::time::Instant;
use tract_onnx::prelude::*;
use ndarray::prelude::*;
use image::DynamicImage;
use tracing::{info, error};

const YOLO_LABELS: &[&str] = &[
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat", "traffic light",
    "fire hydrant", "stop sign", "parking meter", "bench", "bird", "cat", "dog", "horse", "sheep", "cow",
    "elephant", "bear", "zebra", "giraffe", "backpack", "umbrella", "handbag", "tie", "suitcase", "frisbee",
    "skis", "snowboard", "sports ball", "kite", "baseball bat", "baseball glove", "skateboard", "surfboard",
    "tennis racket", "bottle", "wine glass", "cup", "fork", "knife", "spoon", "bowl", "banana", "apple",
    "sandwich", "orange", "broccoli", "carrot", "hot dog", "pizza", "donut", "cake", "chair", "couch",
    "potted plant", "bed", "dining table", "toilet", "tv", "laptop", "mouse", "remote", "keyboard", "cell phone",
    "microwave", "oven", "toaster", "sink", "refrigerator", "book", "clock", "vase", "scissors", "teddy bear",
    "hair drier", "toothbrush"
];

const USB_CLASS_VIDEO: u8 = 0x0E;

// --- Host Implementation for UsbView ---

impl<'a> HostDevice for UsbView<'a> {
    fn init(&mut self) -> Result<(), LibusbError> {
        info!("Initializing wasi-usb host...");
        Ok(())
    }

    fn list_devices(&mut self) -> Result<Vec<(Resource<UsbDevice>, DeviceDescriptor, DeviceLocation)>, LibusbError> {
        let start = Instant::now();
        let devices = self.0.backend.list_devices(&self.0.allowed_usbdevices)?;
        let mut result = Vec::new();
        
        for (dev, desc, loc) in devices {
            let res = self.0.table.push(dev).map_err(|_| LibusbError::Other)?;
            result.push((res, desc, loc));
        }

        self.0.log_call("device::list_devices", start, Some(result.len()));
        Ok(result)
    }
}

impl<'a> HostUsbDevice for UsbView<'a> {
    fn open(&mut self, self_: Resource<UsbDevice>) -> Result<Resource<UsbDeviceHandle>, LibusbError> {
        let start = Instant::now();
        let device = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        let handle = self.0.backend.open(device)?;
        let res = self.0.table.push(handle).map_err(|_| LibusbError::Other)?;
        self.0.log_call("usb_device::open", start, None);
        Ok(res)
    }

    fn get_configuration_descriptor(&mut self, self_: Resource<UsbDevice>, config_index: u8) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.get_configuration_descriptor(device, config_index)
    }

    fn get_configuration_descriptor_by_value(&mut self, self_: Resource<UsbDevice>, config_value: u8) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.get_configuration_descriptor_by_value(device, config_value)
    }

    fn get_active_configuration_descriptor(&mut self, self_: Resource<UsbDevice>) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.get_active_configuration_descriptor(device)
    }

    fn drop(&mut self, rep: Resource<UsbDevice>) -> wasmtime::Result<()> {
        let _ = self.0.table.delete(rep);
        Ok(())
    }
}

impl<'a> HostDeviceHandle for UsbView<'a> {
    fn get_configuration(&mut self, self_: Resource<UsbDeviceHandle>) -> Result<u8, LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.get_configuration(handle)
    }
    fn set_configuration(&mut self, self_: Resource<UsbDeviceHandle>, config: ConfigValue) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.set_configuration(handle, config)
    }
    fn claim_interface(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.claim_interface(handle, ifac)
    }
    fn release_interface(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.release_interface(handle, ifac)
    }
    fn set_interface_altsetting(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8, alt_setting: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.set_interface_alt_setting(handle, ifac, alt_setting)
    }
    fn clear_halt(&mut self, self_: Resource<UsbDeviceHandle>, endpoint: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.clear_halt(handle, endpoint)
    }
    fn reset_device(&mut self, self_: Resource<UsbDeviceHandle>) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.reset_device(handle)
    }
    fn alloc_streams(&mut self, self_: Resource<UsbDeviceHandle>, num_streams: u32, endpoints: Vec<u8>) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.alloc_streams(handle, num_streams, endpoints)
    }
    fn free_streams(&mut self, self_: Resource<UsbDeviceHandle>, endpoints: Vec<u8>) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.free_streams(handle, endpoints)
    }
    fn kernel_driver_active(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<bool, LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.kernel_driver_active(handle, ifac)
    }
    fn detach_kernel_driver(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.detach_kernel_driver(handle, ifac)
    }
    fn attach_kernel_driver(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.attach_kernel_driver(handle, ifac)
    }
    fn new_transfer(&mut self, self_: Resource<UsbDeviceHandle>, xfer_type: TransferType, setup: TransferSetup, buf_size: u32, opts: TransferOptions) -> Result<Resource<UsbTransfer>, LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        let xfer = handle.new_transfer(xfer_type, setup, buf_size, opts)?;
        let res = self.0.table.push(xfer).map_err(|_| LibusbError::Other)?;
        Ok(res)
    }
    fn close(&mut self, self_: Resource<UsbDeviceHandle>) {
        let _ = self.0.table.delete(self_);
    }
    fn drop(&mut self, rep: Resource<UsbDeviceHandle>) -> wasmtime::Result<()> {
        let _ = self.0.table.delete(rep);
        Ok(())
    }
}

// --- Transfers Implementation for UsbView ---

impl<'a> HostTransfers for UsbView<'a> {
    async fn await_transfer(&mut self, xfer: Resource<UsbTransfer>) -> Result<TransferResult, LibusbError> {
        let start = Instant::now();
        let tx = self.table().get(&xfer).map_err(|_| LibusbError::Io)?;
        
        // Polling loop for completion (since we're async in a synchronous context)
        while !tx.completed.load(std::sync::atomic::Ordering::SeqCst) {
             tokio::task::yield_now().await;
        }

        let mut packets = Vec::new();
        if let Some(results) = tx.iso_packet_results.lock().unwrap().clone() {
            for (actual_length, status) in results {
                let status = match status {
                    0 => IsoPacketStatus::Success,
                    1 => IsoPacketStatus::Error,
                    2 => IsoPacketStatus::TimedOut,
                    3 => IsoPacketStatus::Cancelled,
                    4 => IsoPacketStatus::Stall,
                    5 => IsoPacketStatus::NoDevice,
                    6 => IsoPacketStatus::Overflow,
                    _ => IsoPacketStatus::Error,
                };
                packets.push(IsoPacket { actual_length, status });
            }
        }

        let result = TransferResult {
            data: tx.buffer.clone(),
            packets,
        };

        self.0.log_call("transfers::await_transfer", start, Some(result.data.len()));
        Ok(result)
    }
}

impl<'a> HostTransfer for UsbView<'a> {
    fn submit_transfer(&mut self, self_: Resource<UsbTransfer>, data: Vec<u8>) -> Result<(), LibusbError> {
        let xfer = self.0.table.get_mut(&self_).map_err(|_| LibusbError::NotFound)?;
        if !data.is_empty() {
             xfer.buffer = data;
        }
        xfer.submit()
    }
    fn cancel_transfer(&mut self, self_: Resource<UsbTransfer>) -> Result<(), LibusbError> {
        let xfer = self.0.table.get(&self_).map_err(|_| LibusbError::NotFound)?;
        xfer.cancel()
    }
    fn drop(&mut self, rep: Resource<UsbTransfer>) -> wasmtime::Result<()> {
        let _ = self.0.table.delete(rep);
        Ok(())
    }
}

// --- Hotplug Implementation for UsbView ---

impl<'a> HostHotplug for UsbView<'a> {
    fn enable_hotplug(&mut self) -> Result<(), LibusbError> {
        Ok(())
    }
    fn poll_events(&mut self) -> Vec<(Event, Info, Resource<UsbDevice>)> {
        Vec::new()
    }
}

// --- CV Trait Implementation for UsbView ---

impl<'a> HostFrameStream for UsbView<'a> {
    fn new(&mut self, index: u32) -> Resource<FrameStream> {
        self.table().push(FrameStream { index, handle: None, iface_num: 0, ep_addr: 0 }).expect("resource push failed")
    }
    fn read_frame(&mut self, _rep: Resource<FrameStream>) -> Result<Frame, String> {
        Err("Not implemented".to_string())
    }
    fn drop(&mut self, rep: Resource<FrameStream>) -> wasmtime::Result<()> {
        let _ = self.table().delete(rep);
        Ok(())
    }
}

impl<'a> HostObjectDetector for UsbView<'a> {
    fn new(&mut self, model_path: String) -> Resource<ObjectDetector> {
        info!("Initialising YOLO model. Input path: '{}'", model_path);
        
        let path = if model_path.is_empty() {
            "../yolov8n.onnx".to_string()
        } else {
            model_path.clone()
        };

        let model = match tract_onnx::onnx()
            .model_for_path(&path)
            .and_then(|m| m.with_input_fact(0, f32::fact(&[1, 3, 640, 640]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable()) 
        {
            Ok(m) => {
                info!("Successfully loaded and optimised YOLO model from {}", path);
                Some(m)
            },
            Err(e) => {
                error!("Failed to load YOLO model from {}: {:?}", path, e);
                None
            }
        };

        self.table().push(ObjectDetector { model_path, model }).expect("resource push failed")
    }

    fn detect(&mut self, rep: Resource<ObjectDetector>, f: Frame) -> Result<Vec<Detection>, String> {
        let detector = self.table().get(&rep).map_err(|e| e.to_string())?;
        
        let model = match &detector.model {
            Some(m) => m,
            None => return Err("Model not loaded".to_string()),
        };

        // --- Pre-processing ---
        let img = image::RgbImage::from_raw(f.width, f.height, f.data)
            .ok_or_else(|| "Failed to create image from raw bytes".to_string())?;
        let dynamic_img = DynamicImage::ImageRgb8(img);
        
        // Resize to 640x640 (standard YOLOv8 size)
        let resized = dynamic_img.resize_exact(640, 640, image::imageops::FilterType::Triangle);
        
        // Convert to ndarray [1, 3, 640, 640] f32
        let mut tensor = Array4::<f32>::zeros((1, 3, 640, 640));
        for (x, y, rgb) in resized.to_rgb8().enumerate_pixels() {
            tensor[[0, 0, y as usize, x as usize]] = rgb[0] as f32 / 255.0;
            tensor[[0, 1, y as usize, x as usize]] = rgb[1] as f32 / 255.0;
            tensor[[0, 2, y as usize, x as usize]] = rgb[2] as f32 / 255.0;
        }

        // --- Inference ---
        let tract_tensor = Tensor::from(tensor);
        let result = model.run(tract_onnx::prelude::tvec!(tract_tensor.into()))
            .map_err(|e| format!("Inference failed: {:?}", e))?;
        
        let output = result[0].to_array_view::<f32>()
            .map_err(|e| format!("Failed to get output array: {:?}", e))?;

        // YOLOv8 output is [1, 84, 8400]
        // 84 = [x, y, w, h, class0_score, ..., class79_score]
        let output = output.index_axis(Axis(0), 0); // remove batch dim -> [84, 8400]

        // --- Post-processing ---
        let mut detections = Vec::new();
        let conf_threshold = 0.25;

        for i in 0..8400 {
            let col = output.index_axis(Axis(1), i);
            
            // Find max class score
            let mut max_score = 0.0;
            let mut class_id = 0;
            for c in 0..80 {
                let score = col[4 + c];
                if score > max_score {
                    max_score = score;
                    class_id = c;
                }
            }

            if max_score > conf_threshold {
                let cx = col[0];
                let cy = col[1];
                let w = col[2];
                let h = col[3];
                
                // Convert center to corners (still in 640x640 space)
                let x1 = cx - w / 2.0;
                let y1 = cy - h / 2.0;
                
                detections.push(IntermediateDetection {
                    x1, y1, w, h,
                    score: max_score,
                    class_id,
                });
            }
        }

        // Apply NMS
        let final_detections = apply_nms(detections, 0.45);

        // Map back to original image size
        let results = final_detections.into_iter().map(|d| {
            let scale_x = f.width as f32 / 640.0;
            let scale_y = f.height as f32 / 640.0;
            
            Detection {
                label: YOLO_LABELS.get(d.class_id).unwrap_or(&"unknown").to_string(),
                confidence: d.score,
                box_: BoundingBox {
                    origin: Point { x: (d.x1 * scale_x) as u32, y: (d.y1 * scale_y) as u32 },
                    size: Size { width: (d.w * scale_x) as u32, height: (d.h * scale_y) as u32 },
                }
            }
        }).collect();

        Ok(results)
    }

    fn drop(&mut self, rep: Resource<ObjectDetector>) -> wasmtime::Result<()> {
        let _ = self.table().delete(rep);
        Ok(())
    }
}

struct IntermediateDetection {
    x1: f32,
    y1: f32,
    w: f32,
    h: f32,
    score: f32,
    class_id: usize,
}

fn apply_nms(mut detections: Vec<IntermediateDetection>, iou_threshold: f32) -> Vec<IntermediateDetection> {
    detections.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    
    let mut keep = Vec::new();
    let mut suppressed = vec![false; detections.len()];

    for i in 0..detections.len() {
        if suppressed[i] { continue; }
        
        let det_i = &detections[i];
        keep.push(IntermediateDetection {
            x1: det_i.x1, y1: det_i.y1, w: det_i.w, h: det_i.h,
            score: det_i.score, class_id: det_i.class_id
        });

        for j in (i + 1)..detections.len() {
            if suppressed[j] { continue; }
            if iou(det_i, &detections[j]) > iou_threshold {
                suppressed[j] = true;
            }
        }
    }
    keep
}

fn iou(a: &IntermediateDetection, b: &IntermediateDetection) -> f32 {
    let x1 = a.x1.max(b.x1);
    let y1 = a.y1.max(b.y1);
    let x2 = (a.x1 + a.w).min(b.x1 + b.w);
    let y2 = (a.y1 + a.h).min(b.y1 + b.h);

    let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let area_a = a.w * a.h;
    let area_b = b.w * b.h;
    
    intersection / (area_a + area_b - intersection)
}
