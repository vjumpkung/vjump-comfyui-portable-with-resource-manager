#!/usr/bin/env bash
# Cargo runner for x86_64-pc-windows-gnu test executables: puts the 64-bit
# WebView2Loader.dll (built by webview2-com into target-gnu/debug) next to the executable.
exe="$1"; shift
dir="$(dirname "$exe")"
loader="$(cd "$dir/.." 2>/dev/null && pwd)/WebView2Loader.dll"
[ -f "$dir/WebView2Loader.dll" ] || { [ -f "$loader" ] && cp "$loader" "$dir/WebView2Loader.dll"; }
exec "$exe" "$@"
