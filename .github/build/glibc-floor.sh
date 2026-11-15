#!/usr/bin/env bash
# Fails when an ELF file needs a glibc symbol version above GLIBC_FLOOR (default 2.28, decision 0011), naming the file
# and the symbol. Arguments: files, directories (searched) and AppImages (extracted and searched).
set -euo pipefail

floor=${GLIBC_FLOOR:-2.28}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# One line per undefined symbol with a glibc version: "<version> <symbol> <file>".
versions() {
	objdump -T "$1" | awk -v file="$1" '/\*UND\*/ && match($0, /GLIBC_[0-9.]+/) {
		print substr($0, RSTART + 6, RLENGTH - 6), $NF, file
	}'
}

elf_files() {
	find "$@" -type f -print0 | while IFS= read -r -d '' f; do
		[ "$(head -c 4 "$f")" = $'\177ELF' ] && printf '%s\0' "$f"
	done
}

paths=()
for arg in "$@"; do
	case "$arg" in
	*.AppImage)
		appimage=$(realpath "$arg")
		dir="$work/$(basename "$arg")"
		mkdir "$dir"
		(cd "$dir" && "$appimage" --appimage-extract >/dev/null)
		paths+=("$dir/squashfs-root")
		;;
	*) paths+=("$arg") ;;
	esac
done

count=0
while IFS= read -r -d '' f; do
	versions "$f"
	count=$((count + 1))
done < <(elf_files "${paths[@]}") >"$work/versions"
[ "$count" -gt 0 ] || { echo "no ELF file in: $*" >&2; exit 1; }

above=$(cut -d' ' -f1 "$work/versions" | sort -uV | while read -r version; do
	printf '%s\n' "$version" "$floor" | sort -V -C || echo "$version"
done)
max=$(cut -d' ' -f1 "$work/versions" | sort -V | tail -n 1)
echo "$count ELF files, highest glibc symbol version: $max (floor $floor)"
if [ -n "$above" ]; then
	echo "above the floor (version, symbol, file):" >&2
	awk 'NR == FNR { above[$1]; next } $1 in above' - "$work/versions" <<<"$above" | sort -uV >&2
	exit 1
fi
