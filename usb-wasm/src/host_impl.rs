use crate::component::usb::device::{Host as HostDevice, HostUsbDevice, HostDeviceHandle, DeviceLocation};
use crate::component::usb::descriptors::{DeviceDescriptor, ConfigurationDescriptor};
use crate::component::usb::transfers::{
    Host as HostTransfers, HostTransfer,
    IsoPacket, IsoPacketStatus, TransferResult,
    TransferType, TransferSetup, TransferOptions,
};
use crate::component::usb::errors::LibusbError;
use crate::component::usb::configuration::ConfigValue;
use crate::component::usb::usb_hotplug::{Host as HostHotplug, Event, Info};
use crate::component::usb::cv::{HostObjectDetector, Frame, Detection, ObjectDetector};
use crate::{UsbDevice, UsbDeviceHandle, UsbTransfer, MyState};
use wasmtime::component::Resource;

use std::time::Instant;
use tracing::info;
use tract_onnx::prelude::*;
use image::RgbImage;
use ndarray::ArrayViewMutD;

pub struct ObjectDetectorInner {
    pub model: SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>,
}

// --- Device ---

impl HostDevice for MyState {
    fn init(&mut self) -> Result<(), LibusbError> {
        eprintln!("[WASI-USB-HOST] Initializing backend...");
        info!("WASI-USB Host: Backend will initialize on demand.");
        Ok(())
    }

    fn list_devices(&mut self) -> Result<Vec<(Resource<UsbDevice>, DeviceDescriptor, DeviceLocation)>, LibusbError> {
        let start = Instant::now();
        let devices = self.backend.list_devices(&self.allowed_usbdevices)?;
        eprintln!("[WASI-USB-HOST] list_devices: found {} devices", devices.len());
        let mut result = Vec::new();
        for (dev, desc, loc, name) in devices {
            let name_str = name.unwrap_or_else(|| "Unknown Device".to_string());
            eprintln!("  Device: {:04x}:{:04x} - {}", desc.vendor_id, desc.product_id, name_str);
            let res = self.table.push(dev).map_err(|_| LibusbError::Other)?;
            result.push((res, desc, loc));
        }
        self.log_call("device::list_devices", start, Some(result.len()));
        Ok(result)
    }
}

impl HostUsbDevice for MyState {
    fn open(&mut self, self_: Resource<UsbDevice>) -> Result<Resource<UsbDeviceHandle>, LibusbError> {
        let start = Instant::now();
        let device = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        
        // Print device info for confirmation
        let mut desc = std::mem::MaybeUninit::<libusb1_sys::libusb_device_descriptor>::uninit();
        unsafe {
            libusb1_sys::libusb_get_device_descriptor(device.device, desc.as_mut_ptr());
            let d = desc.assume_init();
            eprintln!("[WASI-USB-HOST] open device: VID={:04x} PID={:04x}", d.idVendor, d.idProduct);
        }

        let handle = self.backend.open(device)?;
        eprintln!("[WASI-USB-TRACE] backend.open returned handle, pushing to table...");
        let res = self.table.push(handle).map_err(|_| LibusbError::Other)?;
        eprintln!("[WASI-USB-TRACE] table.push succeeded for handle");
        self.log_call("usb_device::open", start, None);
        Ok(res)
    }

    fn get_configuration_descriptor(&mut self, self_: Resource<UsbDevice>, config_index: u8) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.get_configuration_descriptor(device, config_index)
    }

    fn get_configuration_descriptor_by_value(&mut self, self_: Resource<UsbDevice>, config_value: u8) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.get_configuration_descriptor_by_value(device, config_value)
    }

    fn get_active_configuration_descriptor(&mut self, self_: Resource<UsbDevice>) -> Result<ConfigurationDescriptor, LibusbError> {
        eprintln!("[WASI-USB-TRACE] get_active_configuration_descriptor entry");
        let device = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        let res = self.backend.get_active_configuration_descriptor(device);
        eprintln!("[WASI-USB-TRACE] get_active_configuration_descriptor exit: {:?}", res.is_ok());
        res
    }

    fn drop(&mut self, rep: Resource<UsbDevice>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep);
        Ok(())
    }
}

// --- Device Handle ---

impl HostDeviceHandle for MyState {
    fn get_configuration(&mut self, self_: Resource<UsbDeviceHandle>) -> Result<u8, LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.get_configuration(h)
    }
    fn set_configuration(&mut self, self_: Resource<UsbDeviceHandle>, config: ConfigValue) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.set_configuration(h, config)
    }
    fn claim_interface(&mut self, self_: Resource<UsbDeviceHandle>, iface: u8) -> Result<(), LibusbError> {
        eprintln!("[WASI-USB-TRACE] claim_interface entry: iface={}", iface);
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        let res = self.backend.claim_interface(h, iface);
        if let Err(ref e) = res {
             eprintln!("[WASI-USB-ERROR] claim_interface failed for iface {}: {:?}", iface, e);
        }
        eprintln!("[WASI-USB-TRACE] claim_interface exit: {:?}", res.is_ok());
        res
    }
    fn release_interface(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        eprintln!("[WASI-USB-TRACE] release_interface entry: iface={}", ifac);
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        let res = self.backend.release_interface(h, ifac);
        eprintln!("[WASI-USB-TRACE] release_interface exit: {:?}", res.is_ok());
        res
    }
    fn set_interface_altsetting(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8, alt: u8) -> Result<(), LibusbError> {
        eprintln!("[WASI-USB-TRACE] set_interface_altsetting entry: iface={}, alt={}", ifac, alt);
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        let res = self.backend.set_interface_alt_setting(h, ifac, alt);
        eprintln!("[WASI-USB-TRACE] set_interface_altsetting exit: {:?}", res.is_ok());
        res
    }
    fn clear_halt(&mut self, self_: Resource<UsbDeviceHandle>, endpoint: u8) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.clear_halt(h, endpoint)
    }
    fn reset_device(&mut self, self_: Resource<UsbDeviceHandle>) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.reset_device(h)
    }
    fn alloc_streams(&mut self, self_: Resource<UsbDeviceHandle>, num: u32, endpoints: Vec<u8>) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.alloc_streams(h, num, endpoints)
    }
    fn free_streams(&mut self, self_: Resource<UsbDeviceHandle>, endpoints: Vec<u8>) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.free_streams(h, endpoints)
    }
    fn kernel_driver_active(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<bool, LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.kernel_driver_active(h, ifac)
    }
    fn detach_kernel_driver(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.detach_kernel_driver(h, ifac)
    }
    fn attach_kernel_driver(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.attach_kernel_driver(h, ifac)
    }
    fn new_transfer(
        &mut self,
        self_: Resource<UsbDeviceHandle>,
        xfer_type: TransferType,
        setup: TransferSetup,
        buf_size: u32,
        opts: TransferOptions,
    ) -> Result<Resource<UsbTransfer>, LibusbError> {
        eprintln!("[WASI-USB-TRACE] new_transfer entry: type={:?}, size={}", xfer_type, buf_size);
        let start = Instant::now();
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        eprintln!("[WASI-USB-TRACE] new_transfer found handle: {:?}", h.handle);
        let transfer = h.new_transfer(xfer_type, setup, buf_size, opts)?;
        eprintln!("[WASI-USB-TRACE] h.new_transfer succeeded, pushing to table...");
        let res = self.table.push(transfer).map_err(|_| LibusbError::Other)?;
        eprintln!("[WASI-USB-TRACE] new_transfer exit: Resource={:?}", res);
        self.log_call("usb_transfers::new_transfer", start, None);
        Ok(res)
    }

    fn close(&mut self, self_: Resource<UsbDeviceHandle>) {
        let _ = self.table.delete(self_);
    }
    fn drop(&mut self, rep: Resource<UsbDeviceHandle>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep);
        Ok(())
    }
}

// --- Unified Transfers ---

impl HostTransfers for MyState {
    /// generic await-transfer for ALL transfer types.
    /// Returns data and per-packet results for isochronous transfers.
    fn await_transfer(&mut self, xfer: Resource<UsbTransfer>) -> Result<TransferResult, LibusbError> {
        let start = Instant::now();
        let tx = self.table.get::<UsbTransfer>(&xfer).map_err(|_| LibusbError::Io)?;

        // Spin until the transfer completion flag is set by the background event thread.
        while !tx.completed.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::yield_now();
        }

        let data = tx.buffer.clone();
        let mut packets = Vec::new();

        if let Some(results) = tx.iso_packet_results.lock().unwrap().clone() {
            for (actual_length, status_code) in results {
                let status = match status_code {
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

        self.log_call("transfers::await_transfer", start, Some(data.len()));
        Ok(TransferResult { data, packets })
    }
}

impl HostTransfer for MyState {
    fn submit_transfer(&mut self, self_: Resource<UsbTransfer>, data: Vec<u8>) -> Result<(), LibusbError> {
        eprintln!("[WASI-USB-TRACE] submit_transfer entry: data_len={}", data.len());
        let xfer = self.table.get_mut::<UsbTransfer>(&self_).map_err(|_| LibusbError::NotFound)?;
        if !data.is_empty() {
            xfer.buffer = data;
        }
        let res = xfer.submit();
        eprintln!("[WASI-USB-TRACE] submit_transfer exit: {:?}", res.is_ok());
        res
    }
    fn cancel_transfer(&mut self, self_: Resource<UsbTransfer>) -> Result<(), LibusbError> {
        let xfer = self.table.get::<UsbTransfer>(&self_).map_err(|_| LibusbError::NotFound)?;
        xfer.cancel()
    }
    fn drop(&mut self, rep: Resource<UsbTransfer>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep);
        Ok(())
    }
}

// --- Hotplug ---

impl HostHotplug for MyState {
    fn enable_hotplug(&mut self) -> Result<(), LibusbError> { Ok(()) }
    fn poll_events(&mut self) -> Vec<(Event, Info, Resource<UsbDevice>)> { Vec::new() }
}

impl crate::component::usb::errors::Host for MyState {}
impl crate::component::usb::configuration::Host for MyState {}
impl crate::component::usb::descriptors::Host for MyState {}

// --- CV: Object Detector (host-side YOLO, native performance) ---

use crate::component::usb::cv::{HostFrameStream, FrameStream};

impl crate::component::usb::cv::Host for MyState {
}

pub struct FrameStreamInnerStub;

impl HostFrameStream for MyState {
    fn new(&mut self, _index: u32) -> Resource<FrameStream> {
        panic!("frame-stream is guest-implemented, host should not be constructing it")
    }

    fn read_frame(&mut self, _self_: Resource<FrameStream>) -> Result<Frame, String> {
        Err("frame-stream is guest-implemented".to_string())
    }

    fn drop(&mut self, _rep: Resource<FrameStream>) -> wasmtime::Result<()> {
        Ok(())
    }
}

impl HostObjectDetector for MyState {
    fn new(&mut self, model_path: String) -> Resource<ObjectDetector> {
        let model = tract_onnx::onnx()
            .model_for_path(model_path).expect("Failed to load model")
            .with_input_fact(0, f32::datum_type().fact(&[1, 3, 640, 640]).into()).expect("Failed to set input fact")
            .into_optimized().expect("Failed to optimize model")
            .into_runnable().expect("Failed to create runnable model");
        self.table.push(ObjectDetectorInner { model }).unwrap()
    }

    fn detect(&mut self, self_: Resource<ObjectDetector>, f: Frame) -> Result<Vec<Detection>, String> {
        let inner = self.table.get::<ObjectDetectorInner>(&self_).map_err(|e| format!("{:?}", e))?;
        let img = match RgbImage::from_raw(f.width, f.height, f.data) {
            Some(i) => i,
            None => return Err("Invalid image data".to_string()),
        };
        let resized = image::imageops::resize(&img, 640, 640, image::imageops::FilterType::Triangle);

        let mut input = Tensor::from_shape(&[1, 3, 640, 640], &[0.0f32; 1 * 3 * 640 * 640]).unwrap();
        {
            let mut view: ArrayViewMutD<f32> = input.to_array_view_mut::<f32>().map_err(|e| e.to_string())?;
            for y in 0..640usize {
                for x in 0..640usize {
                    let p = resized.get_pixel(x as u32, y as u32);
                    view[[0, 0, y, x]] = p[0] as f32 / 255.0;
                    view[[0, 1, y, x]] = p[1] as f32 / 255.0;
                    view[[0, 2, y, x]] = p[2] as f32 / 255.0;
                }
            }
        }

        let result = match inner.model.run(tvec!(input.into())) {
            Ok(r) => r,
            Err(e) => return Err(format!("Model run error: {:?}", e)),
        };
        let output = match result[0].to_array_view::<f32>() {
            Ok(v) => v,
            Err(e) => return Err(format!("Array view error: {:?}", e)),
        };

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
                    box_: crate::component::usb::cv::BoundingBox {
                        origin: crate::component::usb::cv::Point { x, y },
                        size: crate::component::usb::cv::Size {
                            width: (bw * f.width as f32) as u32,
                            height: (bh * f.height as f32) as u32,
                        },
                    },
                });
            }
        }
        Ok(nms(detections, 0.45))
    }

    fn drop(&mut self, rep: Resource<ObjectDetector>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep);
        Ok(())
    }
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

fn iou(a: &crate::component::usb::cv::BoundingBox, b: &crate::component::usb::cv::BoundingBox) -> f32 {
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
