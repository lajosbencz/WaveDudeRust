#!/usr/bin/env bash
# Builds the browser app into the folder given (default target/web): the release wasm of waveduderust_slint bound by
# wasm-bindgen into pkg/ beside frontends/waveduderust_slint/web/index.html. The wasm-bindgen CLI must match Cargo.lock's
# crate; `tools/build-web.sh --bindgen-version` prints that version. A tree with changes gets a fresh build id per run
# (WAVEDUDE_BUILD). binaryen's wasm-opt (-O3, names kept) then shrinks the wasm; WAVEDUDE_NO_WASM_OPT=1 skips it
# (decision 0029).
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
locked=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$root/Cargo.lock")
if [ "${1:-}" = "--bindgen-version" ]; then
	echo "$locked"
	exit 0
fi
app=waveduderust
default_out=web
target=${CARGO_TARGET_DIR:-$root/target}
mkdir -p "${1:-$target/$default_out}"
out=$(cd "${1:-$target/$default_out}" && pwd)
# from the repository, so cargo reads its .cargo/config.toml (web-sys's unstable APIs)
cd "$root"
if [ -z "${WAVEDUDE_NO_WASM_OPT:-}" ] && ! command -v wasm-opt > /dev/null; then
	echo "wasm-opt (binaryen) is not installed: see CONTRIBUTING.md, or WAVEDUDE_NO_WASM_OPT=1 to skip it" >&2
	exit 1
fi
installed=$(wasm-bindgen --version | cut -d' ' -f2)
if [ "$locked" != "$installed" ]; then
	echo "wasm-bindgen $installed is installed, Cargo.lock has $locked: cargo install wasm-bindgen-cli --version $locked" >&2
	exit 1
fi
# the build both wasm files name (waveduderust_app::build_id) and the page's URLs carry: the commit, and the time
# when the tree has changes beside it
if [ -z "${WAVEDUDE_BUILD:-}" ]; then
	WAVEDUDE_BUILD=$(git rev-parse --short=12 HEAD 2>/dev/null || echo local)
	if [ "$WAVEDUDE_BUILD" = local ] || [ -n "$(git status --porcelain 2>/dev/null)" ]; then
		WAVEDUDE_BUILD+=".changed.$(date +%s)"
	fi
fi
# it goes into URLs and a sed expression
if ! [[ $WAVEDUDE_BUILD =~ ^[A-Za-z0-9._+-]+$ ]]; then
	echo "WAVEDUDE_BUILD='$WAVEDUDE_BUILD' may hold only letters, digits and . _ + -" >&2
	exit 1
fi
export WAVEDUDE_BUILD
cargo build --release --locked -p waveduderust_slint --bin "$app" --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" "$target/wasm32-unknown-unknown/release/$app.wasm"
if [ -z "${WAVEDUDE_NO_WASM_OPT:-}" ]; then
	# --debuginfo keeps the name section that panic stacks read; the default features cover rustc's wasm32 output
	for wasm in "$out"/pkg/*_bg.wasm; do
		wasm-opt -O3 --debuginfo "$wasm" -o "$wasm"
	done
fi
# the page's download bar needs the wasm's size, which the compressed response does not give
sed -e "s/__WASM_BYTES__/$(wc -c < "$out/pkg/${app}_bg.wasm")/" -e "s/__BUILD__/$WAVEDUDE_BUILD/g" \
	-e "s/__APP__/$app/g" frontends/waveduderust_slint/web/index.html > "$out/index.html"
echo "built $out ($WAVEDUDE_BUILD); serve it over HTTP, e.g. python3 -m http.server -d $out"
