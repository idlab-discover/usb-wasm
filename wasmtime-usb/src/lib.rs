use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView, IoView};
use std::sync::{Arc, Mutex};
use usb_wasm::{MyState, AllowedUSBDevices, LibusbBackend, CallLog};

pub struct HostState {
    pub inner: MyState,
}

impl HostState {
    pub fn new(args: &[impl AsRef<str>], preopen: Option<String>) -> Self {
        let mut wasi_ctx = WasiCtxBuilder::new();
        wasi_ctx.inherit_stdio().inherit_env().args(args);

        if let Some(preopen) = preopen {
            wasi_ctx.preopened_dir(
                &preopen,
                &preopen,
                wasmtime_wasi::DirPerms::all(),
                wasmtime_wasi::FilePerms::all(),
            ).unwrap();
        }

        let table = ResourceTable::new();
        let inner = MyState {
            table,
            wasi_ctx: wasi_ctx.build(),
            allowed_usbdevices: AllowedUSBDevices::Denied(vec![]), // Allow all by default for now
            backend: Box::new(LibusbBackend::new()),
            call_logs: Arc::new(Mutex::new(Vec::new())),
        };

        Self { inner }
    }

    pub fn export_logs(&self, path: &str) -> std::io::Result<()> {
        let logs = self.inner.call_logs.lock().unwrap();
        let mut wtr = csv::Writer::from_path(path)?;
        wtr.write_record(&["function_name", "start_time_ms", "duration_ns", "buffer_size"])?;
        for log in logs.iter() {
            wtr.write_record(&[
                &log.function_name,
                &log.start_time.elapsed().as_millis().to_string(), // Simplified start time
                &log.duration.as_nanos().to_string(),
                &log.buffer_size.map(|s| s.to_string()).unwrap_or_else(|| "0".to_string()),
            ])?;
        }
        wtr.flush()?;
        Ok(())
    }
}

impl IoView for HostState {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.inner.table
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.inner.wasi_ctx
    }
}
