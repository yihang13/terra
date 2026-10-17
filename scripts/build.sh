#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! rustup target list --installed 2>/dev/null | grep -qx 'wasm32-unknown-unknown'; then
  echo "Missing wasm target. Run: rustup target add wasm32-unknown-unknown" >&2
  exit 1
fi

LOCAL_WB="$ROOT/.tools/wasm-bindgen-0.2.106-x86_64-unknown-linux-musl/wasm-bindgen"
if [[ -x "$LOCAL_WB" ]]; then
  WASM_BINDGEN="$LOCAL_WB"
elif command -v wasm-bindgen >/dev/null 2>&1; then
  WASM_BINDGEN="$(command -v wasm-bindgen)"
else
  echo "wasm-bindgen CLI not found." >&2
  echo "Install it with: cargo install wasm-bindgen-cli --version 0.2.106 --locked" >&2
  exit 1
fi

VERSION="$("$WASM_BINDGEN" --version | awk '{print $2}')"
if [[ "$VERSION" != "0.2.106" ]]; then
  echo "wasm-bindgen $VERSION found, but this project pins 0.2.106." >&2
  echo "Install the matching CLI: cargo install wasm-bindgen-cli --version 0.2.106 --locked" >&2
  exit 1
fi

CARGO_ARGS=(build --release --target wasm32-unknown-unknown)
if [[ "${TERRA_OFFLINE:-0}" == "1" ]]; then
  CARGO_ARGS+=(--offline)
fi

echo "==> Compiling Rust to wasm32-unknown-unknown"
cargo "${CARGO_ARGS[@]}"

echo "==> Generating JavaScript bindings"
"$WASM_BINDGEN" \
  --target web \
  --out-dir "$ROOT/web/pkg" \
  --out-name terra \
  "$ROOT/target/wasm32-unknown-unknown/release/terra.wasm"

echo "==> Built web/pkg/terra_bg.wasm ($(du -h "$ROOT/web/pkg/terra_bg.wasm" | cut -f1))"
echo "    Run ./scripts/serve.sh and open http://localhost:4173"
