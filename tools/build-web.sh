#!/usr/bin/env bash
# Builds the browser app into the folder given (default target/web): the release wasm of wavedude-app and of its
# workers, wavedude-run-worker and wavedude-spill-worker, bound by wasm-bindgen into pkg/ beside
# crates/wavedude-app/web/index.html, run-worker.js and spill-worker.js. The wasm-bindgen CLI must match Cargo.lock's
# crate; `tools/build-web.sh --bindgen-version` prints that version. A tree with changes gets a fresh build id per run
# (WAVEDUDE_BUILD), which rebuilds app-state, the app and the run worker.
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
# the build both wasm files name (wavedude_app_state::build_id) and the page's URLs carry: the commit, and the time
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
# one build each: built together, the workers would take the app's features of their shared crates (wgpu's WebGL)
for bin in wavedude-app wavedude-run-worker wavedude-spill-worker; do
	cargo build --release --locked -p "$bin" --bin "$bin" --target wasm32-unknown-unknown
	wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" "$target/wasm32-unknown-unknown/release/$bin.wasm"
done
cp crates/wavedude-app/web/run-worker.js crates/wavedude-app/web/spill-worker.js "$out/"
# the page's download bar needs the wasm's size, which the compressed response does not give
sed -e "s/__WASM_BYTES__/$(wc -c < "$out/pkg/wavedude-app_bg.wasm")/" -e "s/__BUILD__/$WAVEDUDE_BUILD/g" \
	crates/wavedude-app/web/index.html > "$out/index.html"
echo "built $out ($WAVEDUDE_BUILD); serve it over HTTP, e.g. python3 -m http.server -d $out"
