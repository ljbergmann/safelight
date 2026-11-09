#!/bin/sh
# Checks that RAWmakase on a computer reads a library edited by Safelight
# as Safelight wrote it: builds the desktop's command line from
# vendor/rawmakase and runs core's compatibility test against it
# (core/src/compat.rs). Run it after a sync with RAWmakase and before a
# release. Both sides run with an empty HOME, so neither reads profiles
# installed on this Mac.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
home="$(mktemp -d)"
trap 'rm -rf "$home"' EXIT
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
(cd "$root/vendor/rawmakase" && cargo build --bin rawmakase)
cd "$root/core"
HOME="$home" RAWMAKASE_BIN="$root/vendor/rawmakase/target/debug/rawmakase" \
    cargo test compat -- --ignored
