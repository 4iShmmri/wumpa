#!/bin/sh
# Builds the macOS OCR sidecar as a universal binary into src-tauri/binaries/,
# named for every target triple Tauri may ask for.
set -e
cd "$(dirname "$0")"
out=../../src-tauri/binaries
mkdir -p "$out"
swiftc -O -target arm64-apple-macos12 main.swift -o "$out/wumpa-ocr-arm64"
swiftc -O -target x86_64-apple-macos12 main.swift -o "$out/wumpa-ocr-x86"
lipo -create "$out/wumpa-ocr-arm64" "$out/wumpa-ocr-x86" -output "$out/wumpa-ocr-universal-apple-darwin"
rm "$out/wumpa-ocr-arm64" "$out/wumpa-ocr-x86"
for triple in aarch64-apple-darwin x86_64-apple-darwin; do
  cp "$out/wumpa-ocr-universal-apple-darwin" "$out/wumpa-ocr-$triple"
done
echo "built $out/wumpa-ocr-{universal,aarch64,x86_64}-apple-darwin"
