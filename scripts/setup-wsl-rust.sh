#!/usr/bin/env bash
# Isolated toolchain for Linux baseline checks; no global default/PATH changes.
set -euo pipefail
as_tools="${AS_TOOLCHAIN_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/androswitch-tools}"
export CARGO_HOME="$as_tools/cargo"
export RUSTUP_HOME="$as_tools/rustup"
mkdir -p "$as_tools"
if [[ ! -x "$CARGO_HOME/bin/rustup" ]]; then
    installer="$as_tools/rustup-init"
    curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
        https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init -o "$installer"
    installer_hash="$(curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
        https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init.sha256)"
    printf '%s  %s\n' "${installer_hash%% *}" "$installer" | sha256sum --check --status
    chmod +x "$installer"
    "$installer" -y --no-modify-path --profile minimal --default-toolchain 1.97.1
else
    "$CARGO_HOME/bin/rustup" toolchain install 1.97.1 --profile minimal
fi
printf 'CARGO_HOME=%s\nRUSTUP_HOME=%s\n' "$CARGO_HOME" "$RUSTUP_HOME"
"$CARGO_HOME/bin/rustc" +1.97.1 --version
