#!/bin/sh
# Dumps each example through the C# engine (--render) and compares it with `wavedude-cli compare`; the summary.tsv
# files are concatenated into <out-root>/summary.tsv. Examples are name prefixes of the fixtures (default: all).
# STEPS and EVERY set the schedule (600 and 100).
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=${1:?usage: sweep.sh <out-root> [examples...]}
shift
examples=$root/crates/wavedude-core/tests/fixtures/examples
steps=${STEPS:-600}
every=${EVERY:-100}

cargo build --release -p wavedude-cli --manifest-path "$root/Cargo.toml"
cli=${CARGO_TARGET_DIR:-$root/target}/release/wavedude-cli
[ $# -gt 0 ] || set -- ""

mkdir -p "$out"
: > "$out/summary.tsv"
for prefix in "$@"; do
	for project in "$examples/$prefix"*.wdproj; do
		name=$(basename "$project" .wdproj)
		mkdir -p "$out/$name"
		"$here/run.sh" "$project" "$out/$name/csharp" --steps "$steps" --every "$every" --probe-steps --render
		"$cli" compare "$project" "$out/$name/csharp" --out "$out/$name/rust" --steps "$steps" --every "$every"
		cat "$out/$name/rust/summary.tsv" >> "$out/summary.tsv"
	done
done
