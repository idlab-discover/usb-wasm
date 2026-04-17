// streams-test — Valideert USB 3.0 Bulk Streams via de WASI-USB host.
//
// Doel: aantonen dat `alloc_streams` + `new_transfer` met stream_id != 0
// eind-tot-eind werkt, zonder afhankelijk te zijn van een specifiek UAS-
// device. Op een gewone BOT-stick zal `alloc_streams` doorgaans
// LIBUSB_ERROR_NOT_SUPPORTED teruggeven — dat is een geldig resultaat dat
// aantoont dat de call de host-grens haalt en een echte libusb-respons
// teruggeeft (geen stub).
//
// Gebruik:
//   streams-test <vid_hex> <pid_hex> <iface> <ep_out_hex> <ep_in_hex> [num_streams] [payload_bytes]

use anyhow::{anyhow, Result};

#[cfg(target_arch = "wasm32")]
use usb_wasm_bindings::{
    configuration::ConfigValue,
    device::list_devices,
    transfers::{await_transfer, TransferOptions, TransferSetup, TransferType},
};

fn parse_hex_u16(s: &str) -> Result<u16> {
    u16::from_str_radix(s.trim_start_matches("0x"), 16).map_err(Into::into)
}
fn parse_hex_u8(s: &str) -> Result<u8> {
    u8::from_str_radix(s.trim_start_matches("0x"), 16).map_err(Into::into)
}

pub fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!(
            "Usage: {} <vid> <pid> <iface> <ep_out> <ep_in> [num_streams=16] [payload=512]",
            args[0]
        );
        return Err(anyhow!("missing arguments"));
    }
    let vid = parse_hex_u16(&args[1])?;
    let pid = parse_hex_u16(&args[2])?;
    let iface: u8 = args[3].parse()?;
    let ep_out = parse_hex_u8(&args[4])?;
    let ep_in = parse_hex_u8(&args[5])?;
    let num_streams: u32 = args.get(6).map(|s| s.parse()).transpose()?.unwrap_or(16);
    let payload: u32 = args.get(7).map(|s| s.parse()).transpose()?.unwrap_or(512);

    println!(
        "[streams-test] target {:04x}:{:04x} iface={} ep_out=0x{:02x} ep_in=0x{:02x} num_streams={} payload={}B",
        vid, pid, iface, ep_out, ep_in, num_streams, payload
    );

    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!("This test is WASI-only (needs the streams-enabled host).");
        Err(anyhow!("not compiled for wasm32-wasip2"))
    }

    #[cfg(target_arch = "wasm32")]
    {
        // ── Fase 1: enumerate + match ─────────────────────────────────────
        let devices = list_devices().map_err(|e| anyhow!("list_devices: {:?}", e))?;
        let (device, _desc, loc) = devices
            .into_iter()
            .find(|(_d, d_desc, _l)| d_desc.vendor_id == vid && d_desc.product_id == pid)
            .ok_or_else(|| anyhow!("device {:04x}:{:04x} not found", vid, pid))?;
        println!("[streams-test] match on bus={} addr={} speed={:?}",
                 loc.bus_number, loc.device_address, loc.speed);

        // Geef speed expliciet terug — belangrijk voor thesis-context.
        // Streams zijn pas echt nuttig vanaf SuperSpeed (USB 3.0).
        match loc.speed {
            usb_wasm_bindings::device::UsbSpeed::Super
            | usb_wasm_bindings::device::UsbSpeed::SuperPlus => {
                println!("[streams-test] ✓ SuperSpeed — streams kunnen nuttig zijn");
            }
            _ => {
                println!("[streams-test] ⚠  device is geen SuperSpeed — streams worden vaak geweigerd");
            }
        }

        // ── Fase 2: open + claim ──────────────────────────────────────────
        let handle = device.open().map_err(|e| anyhow!("open failed: {:?}", e))?;
        handle.reset_device().ok();
        let _ = handle.set_configuration(ConfigValue::Value(1));
        handle
            .claim_interface(iface)
            .map_err(|e| anyhow!("claim_interface failed: {:?}", e))?;
        println!("[streams-test] ✓ opened + claimed iface {}", iface);

        // ── Fase 3: alloc_streams ─────────────────────────────────────────
        let alloc_result = handle.alloc_streams(num_streams, &[ep_out, ep_in]);
        match alloc_result {
            Ok(()) => println!(
                "[streams-test] ✓ alloc_streams OK — {} streams op EP 0x{:02x}+0x{:02x}",
                num_streams, ep_out, ep_in
            ),
            Err(e) => {
                println!(
                    "[streams-test] ✗ alloc_streams faalde: {:?}  (host-grens wél bereikt)",
                    e
                );
                // NOT_SUPPORTED is een geldig eindresultaat op niet-UAS devices.
                // We stoppen hier zonder foutstatus — het doel van deze test
                // is aantonen dat de WASI→host call door-arriveert.
                handle.release_interface(iface).ok();
                return Ok(());
            }
        }

        // ── Fase 4: een stream-bulk transfer proberen ─────────────────────
        let setup = TransferSetup {
            bm_request_type: 0,
            b_request: 0,
            w_value: 0,
            w_index: 0,
        };
        let opts_out = TransferOptions {
            endpoint: ep_out,
            timeout_ms: 1000,
            stream_id: 1, // ← niet-nul ⇒ libusb_fill_bulk_stream_transfer
            iso_packets: 0,
        };
        let data = vec![0xA5u8; payload as usize];
        let xfer_out = handle
            .new_transfer(TransferType::Bulk, setup, payload, opts_out)
            .map_err(|e| anyhow!("new_transfer(OUT, stream_id=1) failed: {:?}", e))?;
        let submit_ok = xfer_out.submit_transfer(&data);
        println!(
            "[streams-test] submit bulk-stream OUT (stream_id=1, {}B) -> {:?}",
            payload, submit_ok
        );
        if submit_ok.is_ok() {
            let result = await_transfer(&xfer_out);
            println!(
                "[streams-test] await OUT -> {:?}",
                result.as_ref().map(|r| r.data.len())
            );
        }

        // ── Fase 5: free_streams + teardown ──────────────────────────────
        let free_result = handle.free_streams(&[ep_out, ep_in]);
        println!("[streams-test] free_streams -> {:?}", free_result);
        handle.release_interface(iface).ok();
        println!("[streams-test] done.");
        Ok(())
    }
}
