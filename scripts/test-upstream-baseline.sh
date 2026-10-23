#!/usr/bin/env bash
# Test the actual unmodified upstream revision, not the modified fork.
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
as_tools="${AS_TOOLCHAIN_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/androswitch-tools}"
export CARGO_HOME="$as_tools/cargo"
export RUSTUP_HOME="$as_tools/rustup"
export PATH="$CARGO_HOME/bin:$PATH"
export CARGO_TARGET_DIR="$as_tools/target-linux"
export CARGO_BUILD_JOBS="${AS_BUILD_JOBS:-1}"
# Keep test semantics/debug assertions, but avoid debug-symbol memory pressure.
export CARGO_PROFILE_TEST_DEBUG=0
revision=6388281f66d254c8e01a0fdc7a5a9a30239bc762
snapshot="$as_tools/upstream-$revision"
mkdir -p "$snapshot" "$project_root/target/evidence"
if [[ ! -f "$snapshot/.archive-complete" ]]; then
    git -C "$project_root" archive "$revision" | tar -x -C "$snapshot"
    printf '%s\n' "$revision" > "$snapshot/.archive-complete"
fi
cd "$snapshot"
set +e
cargo +1.97.1 test --workspace --locked 2>&1 | tee "$project_root/target/evidence/upstream-linux-baseline.log"
baseline_status=${PIPESTATUS[0]}
set -e
printf '%s\n' "$baseline_status" > "$project_root/target/evidence/upstream-linux-baseline.exit"
exit "$baseline_status"
