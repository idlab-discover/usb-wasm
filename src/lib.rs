use wasmtime_wasi::preview2::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

pub struct HostState {
    wasi_ctx: WasiCtx,
    wasi_table: ResourceTable,
}

impl HostState {
    pub fn new(args: &[impl AsRef<str>]) -> Self {
        let wasi_ctx = WasiCtxBuilder::new().inherit_stdio().args(args).build();
        let wasi_table = ResourceTable::new();
        Self {
            wasi_ctx,
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
