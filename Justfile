lsusb:
    just build-lsusb
    cargo run -- ./out/lsusb.wasm

xbox:
    just build-xbox
    cargo run -- ./out/xbox.wasm

ping *arg:
    just build-ping
    cargo run --release -- --dir=. ./out/ping.wasm -- {{arg}}

control:
    just build-control
    cargo run -- ./out/control.wasm

mass-storage *arg:
    just build-mass-storage
    cargo run --release -- --dir=. ./out/mass-storage.wasm -- {{arg}}

enumerate-devices-go:
    just build-enumerate-devices-go
    cargo run -- ./command-components/enumerate-devices-go/out/main.component.wasm

enumerate-devices-rust:
    just build-enumerate-devices-rust
    cargo run -- ./out/enumerate-devices-rust.wasm

webcam-cv:
    just build-webcam-cv
    cargo run -- ./out/webcam-cv.wasm

ps5-maze:
    just build-ps5-maze
    cargo run -- ./out/ps5-maze.wasm

flamegraph-mass-storage:
    just build-mass-storage
    cargo flamegraph --no-inline --bin wasmtime-usb -- ./out/mass-storage.wasm benchmark

perf-mass-storage:
    just build-mass-storage
    cargo build --release
    perf record --call-graph dwarf -k mono ./target/release/wasmtime-usb  --profile ./out/mass-storage.wasm benchmark

build-runtime:
    cargo build

build-lsusb:
    just regenerate-bindings
    cargo build -p lsusb --target=wasm32-wasip1
    wasm-tools component new ./target/wasm32-wasip1/debug/lsusb.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/lsusb.wasm

build-xbox:
    just regenerate-bindings
    cargo build -p xbox --target=wasm32-wasip1
    wasm-tools component new ./target/wasm32-wasip1/debug/xbox.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/xbox.wasm

build-xbox-maze:
    just regenerate-bindings
    cargo build -p xbox-maze --target=wasm32-wasip1
    wasm-tools component new ./target/wasm32-wasip1/debug/xbox-maze.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/xbox-maze.wasm

build-ping:
    just regenerate-bindings
    cargo build -p ping --release --target=wasm32-wasip1
    wasm-tools component new ./target/wasm32-wasip1/release/ping.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/ping.wasm

build-control:
    just regenerate-bindings
    cargo build -p control --target=wasm32-wasip1
    wasm-tools component new ./target/wasm32-wasip1/debug/control.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/control.wasm

build-mass-storage:
    just regenerate-bindings
    cargo build -p mass-storage --release --target=wasm32-wasip1
    wasm-tools component new ./target/wasm32-wasip1/release/mass-storage.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/mass-storage.wasm

build-enumerate-devices-rust:
    just regenerate-bindings
    cargo build -p enumerate-devices-rust --release --target=wasm32-wasip1
    wasm-tools component new ./target/wasm32-wasip1/release/enumerate-devices-rust.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/enumerate-devices-rust.wasm

SYSROOT := "/Users/sibrenwieme/Documents/Masterproef/usb-wasm/rusb-wasi/examples/wasi-workload/wasi-sysroot"

build-webcam-cv:
    # just regenerate-bindings
    mkdir -p out
    @echo "WARNING: webcam-cv lacks WASI-compatible deps (nokhwa). Building WIP..."
    PKG_CONFIG_DIR="" \
    PKG_CONFIG_LIBDIR="{{SYSROOT}}/usr/lib/pkgconfig:{{SYSROOT}}/usr/share/pkgconfig" \
    PKG_CONFIG_SYSROOT_DIR="{{SYSROOT}}" \
    PKG_CONFIG_ALLOW_CROSS=1 \
    LIBUSB_STATIC=1 \
    cargo build -p webcam-cv --target wasm32-wasip2 --release
    cp target/wasm32-wasip2/release/webcam_cv.wasm out/webcam-cv.wasm

build-ps5-maze:
    just regenerate-bindings
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
