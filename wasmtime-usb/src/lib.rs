use wasmtime_wasi::{DirPerms, FilePerms, ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

pub struct HostState {
    wasi_ctx: WasiCtx,
    wasi_table: ResourceTable,
}

impl HostState {
    pub fn new(args: &[impl AsRef<str>], preopen: Option<String>) -> Self {
        let mut wasi_ctx = WasiCtxBuilder::new();
        wasi_ctx.inherit_stdio().args(args);

        if let Some(preopen) = preopen {
            wasi_ctx.preopened_dir(
                preopen.as_str(),
                preopen.as_str(),
                DirPerms::all(),
                FilePerms::all(),
            ).unwrap();
        }

        let wasi_table = ResourceTable::new();
        Self {
            wasi_ctx: wasi_ctx.build(),
            wasi_table,
        }
    }
}

impl WasiView for HostState {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.wasi_table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
    }
}
