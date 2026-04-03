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
const USB_SUBCLASS_VIDEO_STREAMING: u8 = 0x02;
const UVC_SET_CUR: u8 = 0x01;
const UVC_GET_CUR: u8 = 0x81;
const UVC_VS_PROBE_CONTROL: u16 = 0x0100;
const UVC_VS_COMMIT_CONTROL: u16 = 0x0200;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
    fn new(&mut self, _index: u32) -> Resource<FrameStream> {
        let start = Instant::now();
        info!("Searching for UVC camera...");
        
        let devices = self.0.backend.list_devices(&self.0.allowed_usbdevices).expect("could not list devices");
        let (uvc_device, streaming_iface, ep_addr, alt_setting, max_packet_size) = find_uvc_device(self, &devices)
            .expect("No UVC camera found with streaming interface");

        let handle = self.0.backend.open(&uvc_device).expect("could not open UVC device");
        
        // 1. Handshake: Probe/Commit
        let actual_frame_size = handshake_uvc(self, &handle, streaming_iface).expect("UVC handshake failed");

        // 2. Set Alt Setting to start streaming
        self.0.backend.set_interface_alt_setting(&handle, streaming_iface, alt_setting).expect("could not set alt setting");

        let num_packets = 32;
        let packet_stride = max_packet_size as u32;

        let stream = FrameStream {
            handle,
            iface_num: streaming_iface,
            alt_setting,
            ep_addr,
            max_packet_size,
            actual_frame_size,
            packet_stride,
            num_packets,
            frame_buffer: Arc::new(Mutex::new(Vec::with_capacity(actual_frame_size as usize))),
            last_fid: Arc::new(Mutex::new(0)),
            frame_started: Arc::new(Mutex::new(false)),
            frame_count: Arc::new(Mutex::new(0)),
        };

        let res = self.table().push(stream).expect("resource push failed");
        self.0.log_call("cv::frame_stream::new", start, None);
        res
    }

    fn read_frame(&mut self, rep: Resource<FrameStream>) -> Result<Frame, String> {
        let start = Instant::now();
        let (handle, ep_addr, num_packets, packet_stride, frame_buffer_arc, last_fid_arc, frame_started_arc) = {
            let stream = self.table().get(&rep).map_err(|e| e.to_string())?;
            (
                stream.handle.clone(),
                stream.ep_addr,
                stream.num_packets,
                stream.packet_stride,
                stream.frame_buffer.clone(),
                stream.last_fid.clone(),
                stream.frame_started.clone(),
            )
        };
        
        loop {
            // ISO transfer
            let opts = TransferOptions {
                endpoint: ep_addr,
                timeout_ms: 1000,
                stream_id: 0,
                iso_packets: num_packets,
            };
            
            let xfer = handle.new_transfer(
                TransferType::Isochronous,
                TransferSetup { bm_request_type: 0, b_request: 0, w_value: 0, w_index: 0 },
                num_packets * packet_stride,
                opts
            ).map_err(|e| format!("Xfer alloc failed: {:?}", e))?;

            xfer.submit().map_err(|e| format!("Xfer submit failed: {:?}", e))?;

            // Polling loop for completion
            while !xfer.completed.load(Ordering::SeqCst) {
                 std::thread::sleep(std::time::Duration::from_millis(1));
            }

            // Reassemble
            let results = xfer.iso_packet_results.lock().unwrap().clone().unwrap_or_default();
            let mut data_ptr = 0;
            
            for (actual_length, status) in results {
                if status != 0 { 
                    data_ptr += packet_stride as usize;
                    continue; 
                }
                if actual_length < 2 { 
                    data_ptr += packet_stride as usize;
                    continue; 
                }

                let packet_data = &xfer.buffer[data_ptr..data_ptr + actual_length as usize];
                let header_len = packet_data[0];
                if header_len < 2 || header_len as usize > packet_data.len() {
                    data_ptr += packet_stride as usize;
                    continue;
                }

                let bitfield = packet_data[1];
                let fid = bitfield & 0x01;
                let eof = (bitfield & 0x02) != 0;

                let mut last_fid_lock = last_fid_arc.lock().unwrap();
                let mut frame_started_lock = frame_started_arc.lock().unwrap();
                let mut frame_buffer_lock = frame_buffer_arc.lock().unwrap();

                let payload = &packet_data[header_len as usize..];

                // Additional MJPEG check: SOI (Start of Image)
                let has_soi = payload.len() >= 2 && payload[0] == 0xFF && payload[1] == 0xD8;
                let has_eoi = payload.len() >= 2 && (payload[payload.len()-2] == 0xFF && payload[payload.len()-1] == 0xD9);

                if fid != *last_fid_lock || has_soi {
                    // New frame detected via FID toggle or MJPEG SOI
                    if *frame_started_lock && !frame_buffer_lock.is_empty() {
                        let final_data = frame_buffer_lock.clone();
                        frame_buffer_lock.clear();
                        *last_fid_lock = fid;
                        *frame_started_lock = true;
                        frame_buffer_lock.extend_from_slice(payload);
                        
                        self.0.log_call("cv::frame_stream::read_frame", start, Some(final_data.len()));
                        return Ok(Frame {
                            data: final_data,
                            width: 640,
                            height: 480,
                        });
                    }
                    *frame_started_lock = true;
                    *last_fid_lock = fid;
                    frame_buffer_lock.clear();
                }

                if *frame_started_lock {
                    frame_buffer_lock.extend_from_slice(payload);
                }

                if eof || has_eoi {
                    let final_data = frame_buffer_lock.clone();
                    frame_buffer_lock.clear();
                    *frame_started_lock = false;
                    
                    self.0.log_call("cv::frame_stream::read_frame", start, Some(final_data.len()));
                    return Ok(Frame {
                        data: final_data,
                        width: 640,
                        height: 480,
                    });
                }

                data_ptr += packet_stride as usize;
            }
        }
    }

    fn drop(&mut self, rep: Resource<FrameStream>) -> wasmtime::Result<()> {
        let stream = self.table().delete(rep)?;
        // Set alt setting 0 to stop camera
        let _ = self.0.backend.set_interface_alt_setting(&stream.handle, stream.iface_num, 0);
        self.0.backend.release_interface(&stream.handle, stream.iface_num);
        Ok(())
    }
}

fn find_uvc_device(view: &mut UsbView, devices: &[(UsbDevice, DeviceDescriptor, DeviceLocation)]) -> Option<(UsbDevice, u8, u8, u8, u16)> {
    for (dev, desc, _) in devices {
        // Look for Video Class device
        if desc.device_class == USB_CLASS_VIDEO {
            // Find VideoStreaming interface
            let config = view.0.backend.get_active_configuration_descriptor(dev).ok()?;
            for iface in config.interfaces {
                if iface.interface_class == USB_CLASS_VIDEO && iface.interface_subclass == USB_SUBCLASS_VIDEO_STREAMING {
                    // Find alternate setting with isochronous endpoint
                    // For simplicity, pick one with decent max_packet_size
                    for ep in &iface.endpoints {
                        if (ep.attributes & 0x03) == 0x01 { // Isochronous
                            return Some((*dev, iface.interface_number, ep.endpoint_address, iface.alternate_setting, ep.max_packet_size));
                        }
                    }
                }
            }
        }
    }
    None
}

fn handshake_uvc(view: &mut UsbView, handle: &UsbDeviceHandle, iface: u8) -> Result<u32, String> {
    let mut probe = vec![0u8; 26];
    // 1. Set Interface 0
    view.0.backend.set_interface_alt_setting(handle, iface, 0).map_err(|e| e.to_string())?;
    view.0.backend.claim_interface(handle, iface).map_err(|e| e.to_string())?;

    // 2. Probe GET_CUR
    control_transfer(view, handle, 0xA1, UVC_GET_CUR, UVC_VS_PROBE_CONTROL, iface as u16 + 1, &mut probe)?;
    
    // 3. Probe SET_CUR
    control_transfer(view, handle, 0x21, UVC_SET_CUR, UVC_VS_PROBE_CONTROL, iface as u16 + 1, &mut probe)?;

    // 4. Probe GET_CUR again
    control_transfer(view, handle, 0xA1, UVC_GET_CUR, UVC_VS_PROBE_CONTROL, iface as u16 + 1, &mut probe)?;

    // 5. Commit SET_CUR
    control_transfer(view, handle, 0x21, UVC_SET_CUR, UVC_VS_COMMIT_CONTROL, iface as u16 + 1, &mut probe)?;

    // Extract dwMaxVideoFrameSize (offset 18, 4 bytes)
    let max_frame_size = u32::from_le_bytes([probe[18], probe[19], probe[20], probe[21]]);
    info!("UVC Handshake successful. Max frame size: {}", max_frame_size);

    Ok(max_frame_size)
}

fn control_transfer(_view: &mut UsbView, handle: &UsbDeviceHandle, bm_request_type: u8, b_request: u8, w_value: u16, w_index: u16, data: &mut [u8]) -> Result<(), String> {
    let opts = TransferOptions {
        endpoint: 0,
        timeout_ms: 1000,
        stream_id: 0,
        iso_packets: 0,
    };
    let setup = TransferSetup {
        bm_request_type,
        b_request,
        w_value,
        w_index,
    };
    
    let xfer = handle.new_transfer(TransferType::Control, setup, data.len() as u32, opts).map_err(|e| e.to_string())?;
    xfer.submit().map_err(|e| e.to_string())?;
    
    while !xfer.completed.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    
    if bm_request_type & 0x80 != 0 {
        data.copy_from_slice(&xfer.buffer[8..8+data.len()]);
    }
    
    Ok(())
}


impl<'a> HostObjectDetector for UsbView<'a> {
    fn new(&mut self, model_path: String) -> Resource<ObjectDetector> {
        info!("Initialising YOLO model. Input path: '{}'", model_path);
        
        let path = if model_path.is_empty() {
            if std::path::Path::new("yolov8n.onnx").exists() {
                "yolov8n.onnx".to_string()
            } else if std::path::Path::new("../yolov8n.onnx").exists() {
                "../yolov8n.onnx".to_string()
            } else {
                "yolov8n.onnx".to_string() // Fallback
            }
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
