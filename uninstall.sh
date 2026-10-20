#!/bin/sh
: "${HOME:?HOME is not set}"
rm -f "$HOME/.local/bin/dimtune" "$HOME/.local/share/applications/dimtune.desktop" \
      "$HOME/.local/share/icons/hicolor/scalable/apps/dimtune.svg"
rm -rf "$HOME/.config/dimtune"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "dimtune uninstalled (no leftovers)."
