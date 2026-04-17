JUST := "/opt/homebrew/bin/just"

lsusb:
    {{JUST}} build-lsusb
    cargo build
    sudo ./target/debug/wasmtime-usb ./out/lsusb.wasm

xbox:
    {{JUST}} build-xbox
    cargo build
    sudo ./target/debug/wasmtime-usb ./out/xbox.wasm

ping *arg:
    {{JUST}} build-ping
    cargo build --release
    sudo ./target/release/wasmtime-usb --dir=. ./out/ping.wasm -- {{arg}}

control:
    {{JUST}} build-control
    cargo build
    sudo ./target/debug/wasmtime-usb ./out/control.wasm

mass-storage *arg:
    {{JUST}} build-mass-storage
    cargo build --release
    sudo ./target/release/wasmtime-usb --dir=. ./out/mass-storage.wasm -- {{arg}}

enumerate-devices-go:
    {{JUST}} build-enumerate-devices-go
    cargo run -- ./command-components/enumerate-devices-go/out/main.component.wasm

enumerate-devices-rust:
    {{JUST}} build-enumerate-devices-rust
    cargo build
    sudo ./target/debug/wasmtime-usb ./out/enumerate-devices-rust.wasm

webcam:
    {{JUST}} build-webcam
    cargo build
    sudo ./target/debug/wasmtime-usb ./out/webcam.wasm


yolo:
    {{JUST}} build-yolo-composed
    cargo build
    sudo ./target/debug/wasmtime-usb --dir=. ./out/yolo-composed.wasm


native-webcam:
    cargo run -p webcam-cv

ps5-maze:
    {{JUST}} build-ps5-maze
    cargo build
    sudo ./target/debug/wasmtime-usb ./out/ps5-maze.wasm

flamegraph-mass-storage:
    {{JUST}} build-mass-storage
    cargo flamegraph --no-inline --bin wasmtime-usb -- ./out/mass-storage.wasm benchmark

perf-mass-storage:
    {{JUST}} build-mass-storage
    cargo build --release
    perf record --call-graph dwarf -k mono ./target/release/wasmtime-usb  --profile ./out/mass-storage.wasm benchmark

build-runtime:
    cargo build

build-lsusb:
    {{JUST}} regenerate-bindings
    cargo build -p lsusb --target=wasm32-wasip2
    cp target/wasm32-wasip2/debug/lsusb.wasm out/lsusb.wasm

build-xbox:
    {{JUST}} regenerate-bindings
    cargo build -p xbox --target=wasm32-wasip2
    cp target/wasm32-wasip2/debug/xbox.wasm out/xbox.wasm

build-xbox-maze:
    {{JUST}} regenerate-bindings
    cargo build -p xbox-maze --target=wasm32-wasip2
    cp target/wasm32-wasip2/debug/xbox_maze.wasm out/xbox-maze.wasm

build-ping:
    {{JUST}} regenerate-bindings
    cargo build -p ping --release --target=wasm32-wasip2
    cp target/wasm32-wasip2/release/ping.wasm out/ping.wasm

build-control:
    {{JUST}} regenerate-bindings
    cargo build -p control --target=wasm32-wasip2
    cp target/wasm32-wasip2/debug/control.wasm out/control.wasm

build-mass-storage:
    {{JUST}} regenerate-bindings
    cargo build -p mass-storage --release --target=wasm32-wasip2
    cp target/wasm32-wasip2/release/mass_storage.wasm out/mass-storage.wasm

build-enumerate-devices-rust:
    {{JUST}} regenerate-bindings
    cargo build -p enumerate-devices-rust --release --target=wasm32-wasip2
    cp target/wasm32-wasip2/release/enumerate_devices_rust.wasm out/enumerate-devices-rust.wasm

SYSROOT := "/Users/sibrenwieme/Documents/Masterproef/usb-wasm/rusb-wasi/examples/wasi-workload/wasi-sysroot"

build-webcam:
    {{JUST}} regenerate-bindings
    mkdir -p out
    @echo "Building webcam component..."
    PKG_CONFIG_DIR="" \
    PKG_CONFIG_LIBDIR="{{SYSROOT}}/usr/lib/pkgconfig:{{SYSROOT}}/usr/share/pkgconfig" \
    PKG_CONFIG_SYSROOT_DIR="{{SYSROOT}}" \
    PKG_CONFIG_ALLOW_CROSS=1 \
    LIBUSB_STATIC=1 \
    cargo build -p webcam --target wasm32-wasip2 --release
    cp target/wasm32-wasip2/release/webcam.wasm out/webcam.wasm


build-yolo:
    {{JUST}} regenerate-bindings
    mkdir -p out
    cargo build -p yolo-detector --target wasm32-wasip2 --release
    cp target/wasm32-wasip2/release/yolo-detector.wasm out/yolo-detector.wasm


build-ps5-maze:
    {{JUST}} regenerate-bindings
    mkdir -p out
    PKG_CONFIG_DIR="" \
    PKG_CONFIG_LIBDIR="{{SYSROOT}}/usr/lib/pkgconfig:{{SYSROOT}}/usr/share/pkgconfig" \
    PKG_CONFIG_SYSROOT_DIR="{{SYSROOT}}" \
    PKG_CONFIG_ALLOW_CROSS=1 \
    LIBUSB_STATIC=1 \
    cargo clean -p libusb1-sys
    PKG_CONFIG_DIR="" \
    PKG_CONFIG_LIBDIR="{{SYSROOT}}/usr/lib/pkgconfig:{{SYSROOT}}/usr/share/pkgconfig" \
    PKG_CONFIG_SYSROOT_DIR="{{SYSROOT}}" \
    PKG_CONFIG_ALLOW_CROSS=1 \
    LIBUSB_STATIC=1 \
    cargo build -p ps5-maze --target wasm32-wasip2 --release
    cp target/wasm32-wasip2/release/ps5_maze.wasm out/ps5-maze.wasm

build-enumerate-devices-go:
    cd command-components/enumerate-devices-go && ./build.sh

verify:
    wit-bindgen markdown wit/ --out-dir ./out/wit-md/

regenerate-bindings:
    cd usb-wasm-bindings && ./regenerate-bindings.sh

build-yolo-composed: build-webcam build-yolo
    # Step 3: Compose. wac plug links consumer (yolo-detector) to producer (webcam).
    ~/.cargo/bin/wac plug out/yolo-detector.wasm \
        --plug out/webcam.wasm -o out/yolo-composed.wasm