use cap_std::ambient_authority;
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
                cap_std::fs::Dir::open_ambient_dir(preopen.as_str(), ambient_authority()).unwrap(),
                DirPerms::all(),
                FilePerms::all(),
                preopen.as_str(),
            );
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
