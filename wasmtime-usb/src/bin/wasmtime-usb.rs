use anyhow::Result;
use clap::Parser;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime_usb_cli::HostState;
use wasmtime_wasi::WasiView;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    dir: Option<String>,
    #[clap(short, long)]
    profile: bool,
    command: String,
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    command_args: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    let command_component_path = std::path::Path::new(args.command.as_str());

    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    
    if args.profile {
        config.profiler(wasmtime::ProfilingStrategy::PerfMap);
    }

    let engine = Engine::new(&config)?;
    let mut linker: Linker<HostState> = Linker::new(&engine);
    
    // Register WASI preview 2 imports
    wasmtime_wasi::add_to_linker_async(&mut linker)?;
    
    // Register our WASI-USB imports
    usb_wasm::add_to_linker(&mut linker, |state| &mut state.inner)?;

    let mut command_args = args.command_args.clone();
    command_args.insert(0, args.command.clone());
    
    let state = HostState::new(&command_args, args.dir);
    let mut store = Store::new(&engine, state);

    let component = Component::from_file(&engine, command_component_path)?;
    
    let command = wasmtime_wasi::bindings::Command::instantiate_async(&mut store, &component, &linker).await?;
    
    let result: Result<Result<(), ()>, anyhow::Error> = command.wasi_cli_run().call_run(&mut store).await;
    
    // Task 10: Export analytical logging results
    let _ = store.data().export_logs("call_logs.csv");

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) => Err(anyhow::anyhow!("WASI command failed")),
        Err(e) => {
            if let Some(exit) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                std::process::exit(exit.0);
            }
            Err(e)
        }
    }
}
