#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

cargo fmt --manifest-path "$root/Cargo.toml" --check
cargo clippy --manifest-path "$root/Cargo.toml" --all-targets --all-features -- -D warnings
cargo test --manifest-path "$root/Cargo.toml" --all-features
cargo run --manifest-path "$root/Cargo.toml" --release -- demo
