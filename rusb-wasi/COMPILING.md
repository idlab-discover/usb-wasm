# Compiling WASI-USB Workloads

This guide explains how to compile the `rusb` (Rust) and `libusb` (C) workloads for the WASI-USB environment.

## Prerequisites

- **Rust**: Standard Rust installation (`rustup`).
- **WASI-SDK**: For compiling C code. Assumed to be at `/opt/wasi-sdk`.
- **Cargo Component?**: The Rust build uses `cargo build --target wasm32-wasip2`, so standard cargo with the target installed is sufficient.

## 1. Compiling the Rust Workload (`rusb-wasi`)

The Rust workload is located in `examples/wasi-workload`. It uses a script `build.sh` to handle cross-compilation environment variables.

### Steps:
1. Navigate to the workload directory:
   ```bash
   cd examples/wasi-workload
   ```
2. Run the build script:
   ```bash
   ./build.sh
   ```
   
   This script does the following:
   - Sets `PKG_CONFIG_*` variables to point to the `wasi-sysroot` containing `libusb`.
   - Compiles the project using `cargo build --target wasm32-wasip2 --release`.

### Output:
The compiled component will be at:
`examples/wasi-workload/target/wasm32-wasip2/release/rusb_wasi_workload.wasm`

---

## 2. Compiling the C Workload (`libusb-wasi`)

The C workload is located in `../libusb-wasi/examples`.

### Prerequisites
- `wasm-tools` must be installed and in your PATH.
- `wasi-sdk` installed at `/opt/wasi-sdk`.

### Steps:
1. Navigate to the examples directory:
   ```bash
   cd ../libusb-wasi/examples
   ```
2. Run the build script:
   ```bash
   chmod +x build.sh
   ./build.sh
   ```

### Output:
The compiled component will be at:
`../libusb-wasi/examples/read_device.component.wasm`

---

## 3. Running with the Host Runtime

To run these workloads, you need a WASI-USB compatible host runtime (which you are likely developing).

Assuming you have a host runtime CLI (e.g., `wasi-usb-host`):

**Run Rust Workload:**
```bash
./wasi-usb-host --component-path examples/wasi-workload/target/wasm32-wasip2/release/rusb_wasi_workload.wasm
```

**Run C Workload:**
```bash
./wasi-usb-host --component-path ../libusb-wasi/examples/read_device.component.wasm
```

---

## USB Tree Viewer (lsusb)

To verify USB functionality without permission issues (Mass Storage drivers often claim interfaces), use the `lsusb` examples.

### Rust (`lsusb`)
Source: `rusb-wasi/examples/wasi-workload/examples/lsusb.rs`

**Build:**
The `build.sh` script now builds the `lsusb` example automatically.
```bash
cd ../rusb-wasi/examples/wasi-workload
./build.sh
```

**Run:**
```bash
wasi-usb-host --component-path ../rusb-wasi/examples/wasi-workload/target/wasm32-wasip2/release/examples/lsusb.wasm
```

### C (`lsusb`)
Source: `libusb-wasi/examples/lsusb.c`

**Build:**
The `build.sh` script automatically builds `lsusb` as well.
```bash
cd ../libusb-wasi/examples
./build.sh
```

**Run:**
```bash
wasi-usb-host --component-path ../libusb-wasi/examples/lsusb.component.wasm
```

*Note: Ensure your user account has permissions to access USB devices (e.g., check udev rules on Linux).*
