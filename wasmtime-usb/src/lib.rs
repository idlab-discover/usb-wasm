use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView, IoView};
use usb_wasm::{MyState, AllowedUSBDevices, LibusbBackend};


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
        };

        Self { inner }
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
