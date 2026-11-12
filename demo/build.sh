#!/usr/bin/env bash
# Builds the web demo into <out-dir>, emptied first: the page, the wasm engine package and the example files.
# Needs the wasm32-unknown-unknown target and a wasm-bindgen CLI of the version Cargo.lock pins;
# `demo/build.sh --bindgen-version` prints that version.
set -euo pipefail

usage() { echo "usage: demo/build.sh <out-dir> | --bindgen-version" >&2; exit 2; }
[ $# -eq 1 ] || usage
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)

locked=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[^0-9.]/, "", $0); print; exit }' "$repo/Cargo.lock")
[[ "$locked" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "no wasm-bindgen version in Cargo.lock: '$locked'" >&2; exit 1; }
if [ "$1" = "--bindgen-version" ]; then
	echo "$locked"
	exit 0
fi

[ -n "$1" ] || usage
mkdir -p -- "$1"
out=$(cd "$1" && pwd -P)
if [ "$out" = / ] || [ "$out" = "$repo" ] || [[ "$repo" == "$out"/* ]]; then
	echo "refusing to empty $out" >&2
	exit 1
fi

installed=$(wasm-bindgen --version | awk '{ print $2 }')
if [ "$locked" != "$installed" ]; then
	echo "wasm-bindgen CLI $installed does not match the wasm-bindgen crate $locked of Cargo.lock;" >&2
	echo "install it with: cargo install --locked wasm-bindgen-cli --version $locked" >&2
	exit 1
fi

cd "$repo"
cargo build -p wavedude-wasm --release --target wasm32-unknown-unknown
find "$out" -mindepth 1 -delete
wasm-bindgen --target web --out-dir "$out/pkg" target/wasm32-unknown-unknown/release/wavedude_wasm.wasm
cargo run -q -p wavedude-examples --release -- "$out/examples"
cp demo/index.html demo/demo.js demo/draw.js demo/demo.css "$out/"
