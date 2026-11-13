#!/usr/bin/env bash
# Builds the browser app into the folder given (default target/web): the release wasm of wavedude-app bound by
# wasm-bindgen beside crates/wavedude-app/web/index.html. The wasm-bindgen CLI must match Cargo.lock's crate;
# `tools/build-web.sh --bindgen-version` prints that version.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
locked=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$root/Cargo.lock")
if [ "${1:-}" = "--bindgen-version" ]; then
	echo "$locked"
	exit 0
fi
target=${CARGO_TARGET_DIR:-$root/target}
mkdir -p "${1:-$target/web}"
out=$(cd "${1:-$target/web}" && pwd)
# from the repository, so cargo reads its .cargo/config.toml (web-sys's unstable APIs)
cd "$root"
installed=$(wasm-bindgen --version | cut -d' ' -f2)
if [ "$locked" != "$installed" ]; then
	echo "wasm-bindgen $installed is installed, Cargo.lock has $locked: cargo install wasm-bindgen-cli --version $locked" >&2
	exit 1
fi
cargo build --release --locked -p wavedude-app --bin wavedude-app \
	--target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" \
	"$target/wasm32-unknown-unknown/release/wavedude-app.wasm"
cp crates/wavedude-app/web/index.html "$out/"
echo "built $out; serve it over HTTP, e.g. python3 -m http.server -d $out"
