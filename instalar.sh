#!/bin/sh
# Build and install dimtune for the current user (no sudo, no auto-updates).
set -e
: "${HOME:?HOME is not set}"
cd "$(dirname "$0")"
cargo build --release
mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications" \
         "$HOME/.local/share/icons/hicolor/scalable/apps"
install -m755 target/release/dimtune "$HOME/.local/bin/dimtune"
install -m644 dimtune.svg "$HOME/.local/share/icons/hicolor/scalable/apps/dimtune.svg"
bin="$HOME/.local/bin/dimtune"
bin_escapado=$(printf '%s\n' "$bin" | sed 's/[\\&|]/\\&/g')
sed "s|^Exec=.*|Exec=\"$bin_escapado\"|" dimtune.desktop > "$HOME/.local/share/applications/dimtune.desktop"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Done: look for \"dimtune\" in the application menu or run $bin"
