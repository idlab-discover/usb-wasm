fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target == "wasm32-wasip2" {
        // let sysroot = "/Users/sibrenwieme/Documents/Masterproef/usb-wasm/rusb-wasi/examples/wasi-workload/wasi-sysroot";
        // println!("cargo:rustc-link-arg={}/usr/lib/cguest_component_type.o", sysroot);
    }
}
