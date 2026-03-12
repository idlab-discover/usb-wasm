#!/bin/bash
# test-webcam.sh - Build en run de webcam WASI component

set -e

echo "Building webcam WASI component..."
cargo component build --target wasm32-wasip2 --release

COMPONENT="target/wasm32-wasip2/release/webcam_cv.wasm"

if [ ! -f "$COMPONENT" ]; then
    echo "Component not found: $COMPONENT"
    exit 1
fi

echo "✓ Component built: $COMPONENT"
echo ""

# Zoek een UVC webcam (vendor:product ID)
# Veel voorkomende webcams:
# - Logitech C920: 046d:082d
# - Generic UVC: varies
# Pas aan naar jouw webcam!
WEBCAM_VID_PID="046d:082d"  # Logitech C920 (example)

echo "Running webcam component..."
echo "   (Pass your webcam VID:PID with -d flag if different)"
echo ""

# Run via usb-wasi-host met toegang tot jouw webcam
cd ../../usb-wasi-host  # Adjust path naar je host

cargo run --release -- \
    --component-path "../examples/webcam-cv/$COMPONENT" \
    --use-allow-list \
    -d "$WEBCAM_VID_PID" \
    -l debug

echo ""
echo "Webcam test complete"