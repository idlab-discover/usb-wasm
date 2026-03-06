use rusb::{Context, DeviceHandle, UsbContext};
use std::time::Duration;

#[no_mangle]
#[export_name = "cabi_realloc"]
pub unsafe extern "C" fn cabi_realloc(
    old_ptr: *mut u8,
    old_size: usize,
    _align: usize,
    new_size: usize,
) -> *mut u8 {
    if new_size == 0 {
        return std::ptr::null_mut();
    }
    if old_ptr.is_null() {
        std::alloc::alloc(std::alloc::Layout::from_size_align_unchecked(new_size, _align))
    } else {
        std::alloc::realloc(
            old_ptr,
            std::alloc::Layout::from_size_align_unchecked(old_size, _align),
            new_size,
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn exports_wasi_cli_run_run() -> bool {
    let _ = cabi_realloc as usize;
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        return false;
    }
    true
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn convert_argument(input: &str) -> u16 {
    if input.starts_with("0x") {
        return u16::from_str_radix(input.trim_start_matches("0x"), 16).unwrap();
    }
    u16::from_str_radix(input, 16).unwrap_or_else(|_| input.parse::<u16>().expect("Invalid number"))
}

fn run() -> rusb::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("Usage: usb_io <VID> <PID> <EP_OUT> <EP_IN> [MESSAGE]");
        return Ok(());
    }

    let vid = convert_argument(&args[1]);
    let pid = convert_argument(&args[2]);
    let ep_out = convert_argument(&args[3]) as u8;
    let ep_in = convert_argument(&args[4]) as u8;
    let msg = if args.len() > 5 { &args[5] } else { "Hello WASI-USB Loopback (Rust)!" };

    let context = Context::new()?;
    let mut handle = match open_device(&context, vid, pid) {
        Some(h) => h,
        None => {
            eprintln!("Could not open device {:04x}:{:04x}", vid, pid);
            return Ok(());
        }
    };

    handle.set_auto_detach_kernel_driver(true)?;
    handle.claim_interface(0)?;

    println!("Writing: \"{}\" to EP 0x{:02x}...", msg, ep_out);
    let timeout = Duration::from_secs(1);
    match handle.write_bulk(ep_out, msg.as_bytes(), timeout) {
        Ok(len) => println!("Write success ({} bytes sent)", len),
        Err(e) => eprintln!("Write failed: {}", e),
    }

    let mut buf = [0u8; 256];
    println!("Reading from EP 0x{:02x}...", ep_in);
    match handle.read_bulk(ep_in, &mut buf, timeout) {
        Ok(len) => {
            let received = String::from_utf8_lossy(&buf[..len]);
            println!("Read success ({} bytes): \"{}\"", len, received);
        }
        Err(e) => eprintln!("Read failed: {}", e),
    }

    handle.release_interface(0).ok();
    Ok(())
}

fn open_device<T: UsbContext>(context: &T, vid: u16, pid: u16) -> Option<DeviceHandle<T>> {
    let devices = context.devices().ok()?;
    for device in devices.iter() {
        let device_desc = device.device_descriptor().ok()?;
        if device_desc.vendor_id() == vid && device_desc.product_id() == pid {
            return device.open().ok();
        }
    }
    None
}
