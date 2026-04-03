use crate::bindings::component::usb::descriptors::{DeviceDescriptor, ConfigurationDescriptor};
use crate::bindings::component::usb::device::{DeviceLocation, UsbSpeed};
use crate::bindings::component::usb::errors::LibusbError;
use crate::bindings::component::usb::usb_hotplug::{Event, Info};
use crate::bindings::component::usb::configuration::ConfigValue;
use crate::bindings::component::usb::transfers::{TransferType, TransferSetup, TransferOptions};
use crate::{UsbTransfer, AllowedUSBDevices, USBDeviceIdentifier};

use libusb1_sys::{
    libusb_context, libusb_init, libusb_device, libusb_device_handle, libusb_transfer,
    libusb_get_device_list, libusb_free_device_list, libusb_get_device_descriptor,
    libusb_get_bus_number, libusb_get_device_address, libusb_get_port_number, libusb_get_device_speed,
    libusb_ref_device, libusb_open, libusb_close,
    libusb_get_configuration, libusb_set_configuration, libusb_claim_interface, libusb_release_interface,
    libusb_set_interface_alt_setting, libusb_clear_halt, libusb_reset_device,
    libusb_set_auto_detach_kernel_driver,
    libusb_kernel_driver_active, libusb_detach_kernel_driver, libusb_attach_kernel_driver,
    libusb_has_capability, libusb_hotplug_callback_handle, libusb_hotplug_register_callback,
    libusb_handle_events_timeout_completed,
    libusb_alloc_transfer,
    libusb_fill_bulk_transfer, libusb_fill_interrupt_transfer, libusb_fill_control_transfer, libusb_fill_iso_transfer,
};
use libusb1_sys::constants::{
    LIBUSB_CAP_HAS_HOTPLUG, LIBUSB_HOTPLUG_EVENT_DEVICE_ARRIVED, LIBUSB_HOTPLUG_EVENT_DEVICE_LEFT,
    LIBUSB_HOTPLUG_MATCH_ANY, LIBUSB_HOTPLUG_NO_FLAGS,
};

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::VecDeque;
use std::thread;
// Unused log import removed
use libc::timeval;
use once_cell::sync::Lazy;

static HOTPLUG_QUEUE: Lazy<Mutex<VecDeque<(Event, Info, UsbDevice)>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));

#[derive(Clone, Copy)]
pub struct UsbDevice {
    pub device: *mut libusb1_sys::libusb_device,
}

#[derive(Clone, Copy)]
pub struct UsbDeviceHandle {
    pub handle: *mut libusb1_sys::libusb_device_handle,
}

impl UsbDeviceHandle {
    pub fn new_transfer(&self, xfer_type: TransferType, setup: TransferSetup, buf_size: u32, opts: TransferOptions) -> Result<UsbTransfer, LibusbError> {
        unsafe {
            let num_iso_packets = if xfer_type == TransferType::Isochronous {
                opts.iso_packets as i32
            } else {
                0
            };
            
            let transfer = libusb_alloc_transfer(num_iso_packets);
            if transfer.is_null() {
                return Err(LibusbError::NoMem);
            }

            let mut buffer = vec![0u8; buf_size as usize];
            let completed = Arc::new(AtomicBool::new(false));
            let iso_results = Arc::new(Mutex::new(None));

            let user_data = Box::new(TransferContext {
                completed: completed.clone(),
                iso_results: iso_results.clone(),
            });
            let user_data_ptr = Box::into_raw(user_data) as *mut _;

            match xfer_type {
                TransferType::Bulk => {
                    libusb_fill_bulk_transfer(
                        transfer,
                        self.handle,
                        opts.endpoint,
                        buffer.as_mut_ptr(),
                        buf_size as i32,
                        transfer_callback,
                        user_data_ptr,
                        opts.timeout_ms,
                    );
                }
                TransferType::Interrupt => {
                    libusb_fill_interrupt_transfer(
                        transfer,
                        self.handle,
                        opts.endpoint,
                        buffer.as_mut_ptr(),
                        buf_size as i32,
                        transfer_callback,
                        user_data_ptr,
                        opts.timeout_ms,
                    );
                }
                TransferType::Control => {
                    // Prepend setup packet for control transfers
                    let mut setup_buf = vec![0u8; 8];
                    setup_buf[0] = setup.bm_request_type;
                    setup_buf[1] = setup.b_request;
                    setup_buf[2] = (setup.w_value & 0xFF) as u8;
                    setup_buf[3] = (setup.w_value >> 8) as u8;
                    setup_buf[4] = (setup.w_index & 0xFF) as u8;
                    setup_buf[5] = (setup.w_index >> 8) as u8;
                    setup_buf[6] = (buf_size & 0xFF) as u8;
                    setup_buf[7] = (buf_size >> 8) as u8;
                    
                    let mut full_buf = setup_buf;
                    full_buf.extend_from_slice(&buffer);
                    buffer = full_buf;

                    libusb_fill_control_transfer(
                        transfer,
                        self.handle,
                        buffer.as_mut_ptr(),
                        transfer_callback,
                        user_data_ptr,
                        opts.timeout_ms,
                    );
                }
                TransferType::Isochronous => {
                    libusb_fill_iso_transfer(
                        transfer,
                        self.handle,
                        opts.endpoint,
                        buffer.as_mut_ptr(),
                        buf_size as i32,
                        num_iso_packets,
                        transfer_callback,
                        user_data_ptr,
                        opts.timeout_ms as u32,
                    );
                    // libusb requires packet lengths to be set manually
                    let packets = (*transfer).iso_packet_desc.as_mut_ptr();
                    let packet_size = buf_size / opts.iso_packets;
                    for i in 0..num_iso_packets as usize {
                        (*packets.add(i)).length = packet_size as u32;
                    }
                }
            }

            Ok(UsbTransfer {
                transfer,
                completed,
                buffer,
                buf_len: buf_size,
                iso_packet_results: iso_results,
            })
        }
    }
}

struct TransferContext {
    completed: Arc<AtomicBool>,
    iso_results: Arc<Mutex<Option<Vec<(u32, i32)>>>>,
}

extern "system" fn transfer_callback(transfer: *mut libusb1_sys::libusb_transfer) {
    unsafe {
        let context_ptr = (*transfer).user_data as *mut TransferContext;
        let context = Box::from_raw(context_ptr);
        
        if (*transfer).transfer_type == 1 { // LIBUSB_TRANSFER_TYPE_ISOCHRONOUS
            let num_packets = (*transfer).num_iso_packets as usize;
            let packets = (*transfer).iso_packet_desc.as_ptr();
            let mut results = Vec::with_capacity(num_packets);
            for i in 0..num_packets {
                let p = &*packets.add(i);
                results.push((p.actual_length as u32, p.status as i32));
            }
            *context.iso_results.lock().unwrap() = Some(results);
        }

        context.completed.store(true, Ordering::SeqCst);
    }
}

unsafe impl Send for UsbDevice {}
unsafe impl Sync for UsbDevice {}
unsafe impl Send for UsbDeviceHandle {}
unsafe impl Sync for UsbDeviceHandle {}

/// Trait defining the interface for the USB backend.
pub trait HostUsbBackend: Send + Sync {
    fn init(&mut self) -> Result<(), LibusbError>;
    fn list_devices(&mut self, allowed_devices: &AllowedUSBDevices) -> Result<Vec<(UsbDevice, DeviceDescriptor, DeviceLocation)>, LibusbError>;
    fn enable_hotplug(&mut self, allowed_devices: AllowedUSBDevices) -> Result<(), LibusbError>;
    fn poll_events(&mut self) -> Vec<(Event, Info, UsbDevice)>;

    // Device operations
    fn open(&mut self, device: &UsbDevice) -> Result<UsbDeviceHandle, LibusbError>;
    fn close(&mut self, handle: UsbDeviceHandle);
    
    fn get_configuration(&mut self, handle: &UsbDeviceHandle) -> Result<u8, LibusbError>;
    fn set_configuration(&mut self, handle: &UsbDeviceHandle, config: ConfigValue) -> Result<(), LibusbError>;
    
    fn claim_interface(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<(), LibusbError>;
    fn release_interface(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<(), LibusbError>;
    fn set_interface_alt_setting(&mut self, handle: &UsbDeviceHandle, ifac: u8, alt_setting: u8) -> Result<(), LibusbError>;
    
    fn clear_halt(&mut self, handle: &UsbDeviceHandle, endpoint: u8) -> Result<(), LibusbError>;
    fn reset_device(&mut self, handle: &UsbDeviceHandle) -> Result<(), LibusbError>;
    
    fn kernel_driver_active(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<bool, LibusbError>;
    fn detach_kernel_driver(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<(), LibusbError>;
    fn attach_kernel_driver(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<(), LibusbError>;

    fn alloc_streams(&mut self, handle: &UsbDeviceHandle, num_streams: u32, endpoints: Vec<u8>) -> Result<(), LibusbError>;
    fn free_streams(&mut self, handle: &UsbDeviceHandle, endpoints: Vec<u8>) -> Result<(), LibusbError>;

    // Descriptor operations
    fn get_active_configuration_descriptor(&mut self, device: &UsbDevice) -> Result<ConfigurationDescriptor, LibusbError>;
    fn get_configuration_descriptor(&mut self, device: &UsbDevice, config_index: u8) -> Result<ConfigurationDescriptor, LibusbError>;
    fn get_configuration_descriptor_by_value(&mut self, device: &UsbDevice, config_value: u8) -> Result<ConfigurationDescriptor, LibusbError>;
}

pub struct LibusbBackend {
    context: Option<*mut libusb_context>,
    event_loop_flag: Option<Arc<AtomicBool>>,
    event_thread: Option<thread::JoinHandle<()>>,
    hotplug_enabled: bool,
    hotplug_handle: Option<libusb_hotplug_callback_handle>,
}

unsafe impl Send for LibusbBackend {}
unsafe impl Sync for LibusbBackend {}

impl LibusbBackend {
    pub fn new() -> Self {
        Self {
            context: None,
            event_loop_flag: None,
            event_thread: None,
            hotplug_enabled: false,
            hotplug_handle: None,
        }
    }
}

extern "system" fn hotplug_cb(
    _: *mut libusb_context,
    dev: *mut libusb_device,
    ev: libusb1_sys::libusb_hotplug_event,
    user_data: *mut std::ffi::c_void,
) -> std::os::raw::c_int {
    unsafe {
        let mut desc = std::mem::MaybeUninit::<libusb1_sys::libusb_device_descriptor>::uninit();
        if libusb_get_device_descriptor(dev, desc.as_mut_ptr()) != 0 {
            return 0;
        }
        let desc = desc.assume_init();
        let device_id = USBDeviceIdentifier {
            vendor_id: desc.idVendor,
            product_id: desc.idProduct,
        };
        
        let allowed_devices = &*(user_data as *const Mutex<AllowedUSBDevices>);
        if !allowed_devices.lock().unwrap().is_allowed(&device_id) {
            return 0;
        }
        
        let info = Info {
            bus: libusb_get_bus_number(dev),
            address: libusb_get_device_address(dev),
            vendor: desc.idVendor,
            product: desc.idProduct,
        };
        let event = match ev {
            LIBUSB_HOTPLUG_EVENT_DEVICE_ARRIVED => Event::ARRIVED,
            LIBUSB_HOTPLUG_EVENT_DEVICE_LEFT => Event::LEFT,
            _ => return 0,
        };

        libusb_ref_device(dev); 
        HOTPLUG_QUEUE.lock().unwrap().push_back((event, info, UsbDevice{ device: dev }));
        0
    }
}

impl HostUsbBackend for LibusbBackend {
    fn init(&mut self) -> Result<(), LibusbError> {
        if self.context.is_some() {
            return Ok(());
        }
        unsafe {
            let mut ctx: *mut libusb_context = std::ptr::null_mut();
            let res = libusb_init(&mut ctx);
            if res < 0 {
                return Err(LibusbError::from_raw(res));
            }

            self.context = Some(ctx);

            let flag = Arc::new(AtomicBool::new(true));
            self.event_loop_flag = Some(flag.clone());
            let ctx_num = ctx as usize;
            let handle = thread::spawn(move || {
                let tv = timeval { tv_sec: 0, tv_usec: 20_000 };
                while flag.load(Ordering::SeqCst) {
                    let rc = libusb_handle_events_timeout_completed(ctx_num as *mut libusb_context, &tv, std::ptr::null_mut());
                    if rc < 0 {
                        break;
                    }
                }
            });
            self.event_thread = Some(handle);
            Ok(())
        }
    }

    fn list_devices(&mut self, allowed_devices: &AllowedUSBDevices) -> Result<Vec<(UsbDevice, DeviceDescriptor, DeviceLocation)>, LibusbError> {
        unsafe {
            let mut list_ptr: *mut *mut libusb_device = std::ptr::null_mut();
            let cnt = libusb_get_device_list(
                self.context.ok_or(LibusbError::NotFound)?,
                &mut list_ptr as *mut _ as *mut _,
            );
            if cnt < 0 {
                return Err(LibusbError::from_raw(cnt as i32));
            }
            let mut devices = Vec::new();
            for i in 0..cnt {
                let dev = *list_ptr.add(i as usize);
                if dev.is_null() { continue; }
                
                let mut desc = std::mem::MaybeUninit::<libusb1_sys::libusb_device_descriptor>::uninit();
                if libusb_get_device_descriptor(dev, desc.as_mut_ptr()) < 0 { continue; }
                let device_desc = desc.assume_init();
                
                let usb_device_id = USBDeviceIdentifier {
                    vendor_id: device_desc.idVendor,
                    product_id: device_desc.idProduct,
                };

                if !allowed_devices.is_allowed(&usb_device_id) {
                    continue;
                }

                libusb_ref_device(dev);
                let resource = UsbDevice { device: dev };
                
                let location = DeviceLocation {
                    bus_number: libusb_get_bus_number(dev),
                    device_address: libusb_get_device_address(dev),
                    port_number: libusb_get_port_number(dev),
                    speed: UsbSpeed::from_raw(libusb_get_device_speed(dev) as u8)
                };
                
                let descriptor = DeviceDescriptor {
                    length: device_desc.bLength,
                    descriptor_type: device_desc.bDescriptorType,
                    usb_version_bcd: device_desc.bcdUSB,
                    device_class: device_desc.bDeviceClass,
                    device_subclass: device_desc.bDeviceSubClass,
                    device_protocol: device_desc.bDeviceProtocol,
                    max_packet_size0: device_desc.bMaxPacketSize0,
                    vendor_id: device_desc.idVendor,
                    product_id: device_desc.idProduct,
                    device_version_bcd: device_desc.bcdDevice,
                    manufacturer_index: device_desc.iManufacturer,
                    product_index: device_desc.iProduct,
                    serial_number_index: device_desc.iSerialNumber,
                    num_configurations: device_desc.bNumConfigurations,
                };
                
                devices.push((resource, descriptor, location));
            }
            libusb_free_device_list(list_ptr, 1);
            Ok(devices)
        }
    }

    fn enable_hotplug(&mut self, allowed_devices: AllowedUSBDevices) -> Result<(), LibusbError> {
         if self.hotplug_enabled {
            return Ok(());
        }
        unsafe {
            if libusb_has_capability(LIBUSB_CAP_HAS_HOTPLUG) == 0 {
                return Err(LibusbError::NotSupported);
            }

            let allowed_devices = Arc::new(Mutex::new(allowed_devices));
            let user_data = Arc::into_raw(allowed_devices) as *mut std::ffi::c_void;

            let mut handle: libusb_hotplug_callback_handle = 0;
            let rc = libusb_hotplug_register_callback(
                self.context.ok_or(LibusbError::NotFound)?,
                LIBUSB_HOTPLUG_EVENT_DEVICE_ARRIVED | LIBUSB_HOTPLUG_EVENT_DEVICE_LEFT,
                LIBUSB_HOTPLUG_NO_FLAGS,
                LIBUSB_HOTPLUG_MATCH_ANY,
                LIBUSB_HOTPLUG_MATCH_ANY,
                LIBUSB_HOTPLUG_MATCH_ANY,
                hotplug_cb,
                user_data,
                &mut handle,
            );
            if rc < 0 {
                return Err(LibusbError::from_raw(rc));
            }
            self.hotplug_handle = Some(handle);
            self.hotplug_enabled = true;
        }
        Ok(())
    }

    fn poll_events(&mut self) -> Vec<(Event, Info, UsbDevice)> {
        let mut q = HOTPLUG_QUEUE.lock().unwrap();
        let mut out = Vec::with_capacity(q.len());
        while let Some(ev) = q.pop_front() {
            out.push(ev);
        }
        out
    }

    fn open(&mut self, device: &UsbDevice) -> Result<UsbDeviceHandle, LibusbError> {
        let device_ptr = device.device;
        unsafe {
            let mut handle_ptr: *mut libusb_device_handle = std::ptr::null_mut();
            let res = libusb_open(device_ptr, &mut handle_ptr);
            if res < 0 {
                return Err(LibusbError::from_raw(res));
            }
            let _ = libusb_set_auto_detach_kernel_driver(handle_ptr, 1);
            Ok(UsbDeviceHandle { handle: handle_ptr })
        }
    }

    fn close(&mut self, handle: UsbDeviceHandle) {
        unsafe {
            libusb_close(handle.handle);
        }
    }

    fn get_configuration(&mut self, handle: &UsbDeviceHandle) -> Result<u8, LibusbError> {
        unsafe {
            let mut config: i32 = 0;
            let res = libusb_get_configuration(handle.handle, &mut config);
            match res {
                0.. => Ok(config as u8),
                _ => Err(LibusbError::from_raw(res)),
            }
        }
    }

    fn set_configuration(&mut self, handle: &UsbDeviceHandle, config: ConfigValue) -> Result<(), LibusbError> {
        unsafe {
            let config_value = match config {
                ConfigValue::Value(value) => value as i32,
                ConfigValue::Unconfigured => 0,
            };
            let res = libusb_set_configuration(handle.handle, config_value);
            match res {
                0.. => Ok(()),
                _ => Err(LibusbError::from_raw(res)),
            }
        }
    }

    fn claim_interface(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<(), LibusbError> {
        unsafe {
            let res = libusb_claim_interface(handle.handle, ifac as i32);
            if res < 0 {
                return Err(LibusbError::from_raw(res));
            }
            Ok(())
        }
    }

    fn release_interface(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<(), LibusbError> {
        unsafe {
            let res = libusb_release_interface(handle.handle, ifac as i32);
             match res {
                0.. => Ok(()),
                _ => Err(LibusbError::from_raw(res)),
            }
        }
    }

    fn set_interface_alt_setting(&mut self, handle: &UsbDeviceHandle, ifac: u8, alt_setting: u8) -> Result<(), LibusbError> {
        unsafe {
            let res = libusb_set_interface_alt_setting(handle.handle, ifac as i32, alt_setting as i32);
             match res {
                0.. => Ok(()),
                _ => Err(LibusbError::from_raw(res)),
            }
        }
    }

    fn clear_halt(&mut self, handle: &UsbDeviceHandle, endpoint: u8) -> Result<(), LibusbError> {
         unsafe {
            let res = libusb_clear_halt(handle.handle, endpoint);
             match res {
                0.. => Ok(()),
                _ => Err(LibusbError::from_raw(res)),
            }
        }
    }

    fn reset_device(&mut self, handle: &UsbDeviceHandle) -> Result<(), LibusbError> {
         unsafe {
            let res = libusb_reset_device(handle.handle);
             match res {
                0.. => Ok(()),
                _ => Err(LibusbError::from_raw(res)),
            }
        }
    }

    fn kernel_driver_active(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<bool, LibusbError> {
        unsafe {
            let res = libusb_kernel_driver_active(handle.handle, ifac as i32);
            match res {
                0 => Ok(false),
                1.. => Ok(true),
                _ => Err(LibusbError::from_raw(res)),
            }
        }
    }

    fn detach_kernel_driver(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<(), LibusbError> {
         unsafe {
            let res = libusb_detach_kernel_driver(handle.handle, ifac as i32);
             match res {
                0.. => Ok(()),
                _ => Err(LibusbError::from_raw(res)),
            }
        }
    }

    fn attach_kernel_driver(&mut self, handle: &UsbDeviceHandle, ifac: u8) -> Result<(), LibusbError> {
         unsafe {
            let res = libusb_attach_kernel_driver(handle.handle, ifac as i32);
             match res {
                0.. => Ok(()),
                _ => Err(LibusbError::from_raw(res)),
            }
        }
    }

    fn alloc_streams(&mut self, _handle: &UsbDeviceHandle, _num_streams: u32, _endpoints: Vec<u8>) -> Result<(), LibusbError> {
        Err(LibusbError::NotSupported)
    }

    fn free_streams(&mut self, _handle: &UsbDeviceHandle, _endpoints: Vec<u8>) -> Result<(), LibusbError> {
        Err(LibusbError::NotSupported)
    }

    fn get_active_configuration_descriptor(&mut self, device: &UsbDevice) -> Result<ConfigurationDescriptor, LibusbError> {
        unsafe {
            let mut config_desc: *const libusb1_sys::libusb_config_descriptor = std::ptr::null();
            let res = libusb1_sys::libusb_get_active_config_descriptor(device.device, &mut config_desc);
            if res < 0 {
                return Err(LibusbError::from_raw(res));
            }
            let descriptor = generate_config_descriptor(&*config_desc);
            libusb1_sys::libusb_free_config_descriptor(config_desc);
            Ok(descriptor)
        }
    }

    fn get_configuration_descriptor(&mut self, device: &UsbDevice, config_index: u8) -> Result<ConfigurationDescriptor, LibusbError> {
        unsafe {
             let mut config_desc: *const libusb1_sys::libusb_config_descriptor = std::ptr::null();
            let res = libusb1_sys::libusb_get_config_descriptor(device.device, config_index, &mut config_desc);
            if res < 0 {
                return Err(LibusbError::from_raw(res));
            }
            let descriptor = generate_config_descriptor(&*config_desc);
            libusb1_sys::libusb_free_config_descriptor(config_desc);
            Ok(descriptor)
        }
    }

    fn get_configuration_descriptor_by_value(&mut self, device: &UsbDevice, config_value: u8) -> Result<ConfigurationDescriptor, LibusbError> {
         unsafe {
            let mut config_desc: *const libusb1_sys::libusb_config_descriptor = std::ptr::null();
            let res = libusb1_sys::libusb_get_config_descriptor_by_value(device.device, config_value, &mut config_desc);
            if res < 0 {
                return Err(LibusbError::from_raw(res));
            }
            let descriptor = generate_config_descriptor(&*config_desc);
            libusb1_sys::libusb_free_config_descriptor(config_desc);
            Ok(descriptor)
        }
    }
}

unsafe fn generate_config_descriptor(raw_descriptor: &libusb1_sys::libusb_config_descriptor) -> ConfigurationDescriptor {
    use crate::usb::descriptors::{InterfaceDescriptor, EndpointDescriptor};
    let mut interfaces: Vec<InterfaceDescriptor> = Vec::new();
    for i in 0..raw_descriptor.bNumInterfaces {
        let interface = &*raw_descriptor.interface.wrapping_add(i as usize);
        for j in 0..interface.num_altsetting {
            let alt_setting = &*interface.altsetting.wrapping_add(j as usize);
            let mut endpoints: Vec<EndpointDescriptor> = Vec::new();
            for k in 0..alt_setting.bNumEndpoints {
                let endpoint = &*alt_setting.endpoint.wrapping_add(k as usize);
                let endpoint_desc = EndpointDescriptor {
                    length: endpoint.bLength,
                    descriptor_type: endpoint.bDescriptorType,
                    endpoint_address: endpoint.bEndpointAddress,
                    attributes: endpoint.bmAttributes,
                    max_packet_size: endpoint.wMaxPacketSize,
                    interval: endpoint.bInterval,
                    refresh: endpoint.bRefresh,
                    synch_address: endpoint.bSynchAddress,
                };
                endpoints.push(endpoint_desc);
            }
            let interface_desc = InterfaceDescriptor {
                length: alt_setting.bLength,
                descriptor_type: alt_setting.bDescriptorType,
                interface_number: alt_setting.bInterfaceNumber,
                alternate_setting: alt_setting.bAlternateSetting,
                interface_class: alt_setting.bInterfaceClass,
                interface_subclass: alt_setting.bInterfaceSubClass,
                interface_protocol: alt_setting.bInterfaceProtocol,
                interface_index: alt_setting.iInterface,
                endpoints,
            };
            interfaces.push(interface_desc);
        }
    }

    ConfigurationDescriptor {
        length: raw_descriptor.bLength,
        descriptor_type: raw_descriptor.bDescriptorType,
        total_length: raw_descriptor.wTotalLength,
        configuration_value: raw_descriptor.bConfigurationValue,
        configuration_index: raw_descriptor.iConfiguration,
        attributes: raw_descriptor.bmAttributes,
        max_power: raw_descriptor.bMaxPower,
        interfaces
    }
}
