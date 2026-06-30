#!/bin/sh
# paxc — uninstaller
#
# Removes the binaries installed by install.sh. paxc ships two: the compiler
# (paxc) and its companion interpreter (paxr). Neither stores anything else on
# disk (no config, no history), so this is the entire cleanup.
#
#     curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/excelano/paxc/main/uninstall.sh | sh

set -eu

if [ -n "${CARGO_HOME:-}" ]; then
    install_dir="$CARGO_HOME/bin"
else
    install_dir="$HOME/.cargo/bin"
fi

removed=0
for bin in paxc paxr; do
    target="$install_dir/$bin"
    if [ -e "$target" ]; then
        rm -f "$target"
        echo "Removed $target"
        removed=1
    elif command -v "$bin" >/dev/null 2>&1; then
        found="$(command -v "$bin")"
        echo "$bin is installed at $found, not the expected location ($target)."
        echo "Remove it manually if you want it gone."
    fi
done

if [ "$removed" -eq 0 ]; then
    echo "paxc is not installed."
fi
