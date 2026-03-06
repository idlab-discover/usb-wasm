use std::path::PathBuf;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS") == Ok("wasi".into()) {
        // Tell the linker to include cguest_component_type.o, which contains
        // the "component-type:cguest" custom section describing the WASI-USB
        // WIT world. Without this, wasm-component-ld cannot resolve the custom
        // component:usb/* imports from libusb-wasi.
        let sysroot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("wasi-sysroot/usr/lib");
        println!("cargo:rustc-link-arg={}", sysroot.join("cguest_component_type.o").display());
    }
}
