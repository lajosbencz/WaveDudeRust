#!/usr/bin/env bash
# Builds the browser app into the folder given (default target/web): the release wasm of waveduderust_slint and of its
# workers, wavedude-run-worker (decision 0024) and wavedude-spill-worker (decision 0020), bound by wasm-bindgen into
# pkg/ (the run worker's 128-bit SIMD build into pkg-simd/, decision 0034) beside frontends/waveduderust_slint/web/index.html, run-worker.js and spill-worker.js. The wasm-bindgen CLI must
# match Cargo.lock's crate; `tools/build-web.sh --bindgen-version` prints that version. A tree with changes gets a fresh
# build id per run (WAVEDUDE_BUILD), which rebuilds the app and the workers. binaryen's wasm-opt (-O3, names kept) then shrinks each
# wasm; WAVEDUDE_NO_WASM_OPT=1 skips it (decision 0029).
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
# the build both wasm files name (waveduderust_remote::build_id) and the page's URLs carry: the commit, and the time
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
# the run worker again with 128-bit SIMD (decision 0034), beside the builds below: its own target dir, so neither waits
# on the other's lock nor rebuilds the other's crates for the flag
mkdir -p "$target"
simd_log="$target/web-simd.log"
(
	cargo build --release --locked -p waveduderust_workers --bin wavedude-run-worker --target wasm32-unknown-unknown \
		--target-dir "$target/simd" --config 'target.wasm32-unknown-unknown.rustflags=["-C", "target-feature=+simd128"]'
	wasm-bindgen --target web --no-typescript --out-dir "$out/pkg-simd" \
		"$target/simd/wasm32-unknown-unknown/release/wavedude-run-worker.wasm"
) > "$simd_log" 2>&1 &
simd=$!
trap 'pkill -P "$simd" 2> /dev/null || :; kill "$simd" 2> /dev/null || :' EXIT
# one build each: built together, the workers would take the app's features of their shared crates (wgpu's WebGL)
for bin in "$app" wavedude-run-worker wavedude-spill-worker; do
	case $bin in wavedude-*-worker) package=waveduderust_workers ;; *) package=waveduderust_slint ;; esac
	cargo build --release --locked -p "$package" --bin "$bin" --target wasm32-unknown-unknown
	wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" "$target/wasm32-unknown-unknown/release/$bin.wasm"
done
if ! wait "$simd"; then
	cat "$simd_log" >&2
	exit 1
fi
if [ -z "${WAVEDUDE_NO_WASM_OPT:-}" ]; then
	# --debuginfo keeps the name section that panic stacks read; the default features and the modules' target features
	# section cover rustc's wasm32 output
	opts=()
	for wasm in "$out"/pkg/*_bg.wasm "$out"/pkg-simd/*_bg.wasm; do
		wasm-opt -O3 --debuginfo "$wasm" -o "$wasm" &
		opts+=($!)
	done
	for pid in "${opts[@]}"; do
		wait "$pid"
	done
fi
cp frontends/waveduderust_slint/web/run-worker.js frontends/waveduderust_slint/web/spill-worker.js "$out/"
# the page's download bar needs the wasm's size, which the compressed response does not give
sed -e "s/__WASM_BYTES__/$(wc -c < "$out/pkg/${app}_bg.wasm")/" -e "s/__BUILD__/$WAVEDUDE_BUILD/g" \
	-e "s/__APP__/$app/g" frontends/waveduderust_slint/web/index.html > "$out/index.html"
echo "built $out ($WAVEDUDE_BUILD); serve it over HTTP, e.g. python3 -m http.server -d $out"
