#!/bin/sh
# Installs the programs, menu entry, icons and file types of this folder under PREFIX (default ~/.local);
# `./install.sh --uninstall` removes them.
set -e
here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}
cd "$here"
programs="bin/wavedude-app bin/wavedude-cli"
data="share/applications/wavedude-app.desktop share/mime/packages/wavedude-rust.xml $(find share/icons -type f)"
if [ "$1" = --uninstall ]; then
	for f in $programs $data; do rm -f "$prefix/$f"; done
else
	for f in $programs; do install -D -m 755 "$f" "$prefix/$f"; done
	for f in $data; do install -D -m 644 "$f" "$prefix/$f"; done
	# the menu's session may not have PREFIX/bin on its PATH
	sed -i "s|^Exec=wavedude-app|Exec=$prefix/bin/wavedude-app|" "$prefix/share/applications/wavedude-app.desktop"
fi
update-mime-database "$prefix/share/mime" 2>/dev/null || true
update-desktop-database "$prefix/share/applications" 2>/dev/null || true
gtk-update-icon-cache -q "$prefix/share/icons/hicolor" 2>/dev/null || true
