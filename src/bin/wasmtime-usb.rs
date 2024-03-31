use anyhow::anyhow;
use tracing_subscriber::EnvFilter;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime_usb_cli::HostState;
use wasmtime_wasi::{I32Exit, WasiView};

fn main() -> anyhow::Result<()> {
    // Set up logging
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    
    // TODO create a proper CLI here
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <command component>", args[0]);
        return Ok(());
    }

    let command_component_path = std::path::Path::new(&args[1]);

    // Configure an `Engine` and link in all the host components (Wasi preview 2 and our USB component)
    let config = {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.profiler(wasmtime::ProfilingStrategy::PerfMap);
        config
    };
    let engine = Engine::new(&config)?;
    let mut linker: Linker<HostState> = wasmtime::component::Linker::new(&engine);
    register_host_components(&mut linker)?;

    // Set up the Store with the command line arguments
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut store = Store::new(&engine, HostState::new(&args));

    // Load the component (should be an instance of the wasi command component)
    let component = Component::from_file(&engine, command_component_path)?;
    let (bindings, _instance) =
        wasmtime_wasi::command::sync::Command::instantiate(&mut store, &component, &linker)?;

    // Here our `greet` function doesn't take any parameters for the component,
    // but in the Wasmtime embedding API the first argument is always a `Store`.
    let result = bindings.wasi_cli_run().call_run(&mut store);
    // .expect("failed to invoke 'run' function");

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) => Err(anyhow!("inner error")), // IDK HOW THIS IS CAUSED
        Err(e) => {
            if let Some(source) = e.source() {
                if let Some(exit_code) = source.downcast_ref::<I32Exit>() {
                    std::process::exit(exit_code.process_exit_code());
                    // return Err(exit_code.into());
                }
                println!("Source: {}", source);
            }
            println!("e: {}", e);
            Ok(())
        }
    }
}

fn register_host_components<T: WasiView>(linker: &mut Linker<T>) -> anyhow::Result<()> {
    wasmtime_wasi::command::sync::add_to_linker(linker)?;
    usb_wasm::add_to_linker(linker)?;

    Ok(())
}
