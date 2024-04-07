lsusb:
    just build-lsusb
    cargo run -- ./out/lsusb.wasm

xbox:
    just build-xbox
    cargo run -- ./out/xbox.wasm

ping:
    just build-ping
    cargo run -- ./out/ping.wasm

control:
    just build-control
    cargo run -- ./out/control.wasm

mass-storage *arg:
    just build-mass-storage
    cargo run --release -- ./out/mass-storage.wasm {{arg}}

flamegraph-mass-storage:
    just build-mass-storage
    cargo flamegraph --no-inline --bin wasmtime-usb -- ./out/mass-storage.wasm benchmark

perf-mass-storage:
    just build-mass-storage
    cargo build --release
    perf record --call-graph dwarf -k mono ./target/release/wasmtime-usb ./out/mass-storage.wasm benchmark

build-lsusb:
    just regenerate-bindings
    cargo build -p lsusb --target=wasm32-wasi
    wasm-tools component new ./target/wasm32-wasi/debug/lsusb.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/lsusb.wasm

build-xbox:
    just regenerate-bindings
    cargo build -p xbox --target=wasm32-wasi
    wasm-tools component new ./target/wasm32-wasi/debug/xbox.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/xbox.wasm

build-ping:
    just regenerate-bindings
    cargo build -p ping --target=wasm32-wasi
    wasm-tools component new ./target/wasm32-wasi/debug/ping.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/ping.wasm

build-control:
    just regenerate-bindings
    cargo build -p control --target=wasm32-wasi
    wasm-tools component new ./target/wasm32-wasi/debug/control.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/control.wasm

build-mass-storage:
    just regenerate-bindings
    cargo build -p mass-storage --release --target=wasm32-wasi
    wasm-tools component new ./target/wasm32-wasi/release/mass-storage.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/mass-storage.wasm

verify:
    wit-bindgen markdown wit/ --out-dir ./out/wit-md/

regenerate-bindings:
    cd usb-wasm-bindings && ./regenerate-bindings.sh