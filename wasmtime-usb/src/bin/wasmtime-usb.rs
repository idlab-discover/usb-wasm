use anyhow::anyhow;
use clap::Parser;
use tracing_subscriber::EnvFilter;
// use usb_wasm::error::UsbWasmError;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime_usb_cli::HostState;
use wasmtime_wasi::{I32Exit, WasiView};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    dir: Option<String>,
    command: String,
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    command_args: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    // Set up logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    let command_component_path = std::path::Path::new(args.command.as_str());

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
    let mut command_args = args.command_args;
    command_args.insert(0, args.command.clone());
    let mut store = Store::new(&engine, HostState::new(&command_args, args.dir));

    // Load the component (should be an instance of the wasi command component)
    let component = Component::from_file(&engine, command_component_path)?;

    let (command, _instance) = wasmtime_wasi::bindings::sync::Command::instantiate(&mut store, &component, &linker)?;
    let result = command.wasi_cli_run().call_run(&mut store);

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) => Err(anyhow!("inner error")), // IDK HOW THIS IS CAUSED
        Err(e) => {
            if let Some(source) = e.source() {
                if let Some(exit_code) = source.downcast_ref::<I32Exit>() {
                    std::process::exit(exit_code.process_exit_code());
                    // return Err(exit_code.into());
                }
                
                // if let Some(error) = source.downcast_ref::<UsbWasmError>() {
                //     match error {
                //         UsbWasmError::RusbError(err) => {
                //             println!("{}", err);
                //         }
                //         _ => {
                //             println!("{}", error);
                //         }
                //     }
                //     // return Err(exit_code.into());
                // }
                println!("Source: {}", source);
            }
            println!("e: {}", e);
            Ok(())
        }
    }
}

fn register_host_components<T: WasiView>(linker: &mut Linker<T>) -> anyhow::Result<()> {
    wasmtime_wasi::add_to_linker_sync(linker)?;
    // usb_wasm::add_to_linker(linker)?;

    Ok(())
}
