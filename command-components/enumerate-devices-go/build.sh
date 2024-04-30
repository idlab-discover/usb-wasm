mkdir -p out
go generate
tinygo build -target=wasi -o ./out/main.wasm main.go
wasm-tools component embed --world bindings ../../wit ./out/main.wasm -o ./out/main.embed.wasm # create a component
wasm-tools component new ./out/main.embed.wasm --adapt ../wasi_snapshot_preview1.command.wasm -o ./out/main.component.wasm
wasm-tools validate ./out/main.component.wasm --features component-model