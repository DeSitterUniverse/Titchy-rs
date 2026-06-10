#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

cargo run \
    --manifest-path "$root/Cargo.toml" \
    --release \
    -- benchmark \
    --samples 250000 \
    --iterations 5 \
    --access-operations 10000 \
    --markdown "$root/docs/benchmarks.md"
