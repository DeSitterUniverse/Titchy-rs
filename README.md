# Titchy-rs

Titchy-rs is a lossless compressor for fixed-width sensor time series. It
implements the Titchy algorithm in Rust with bounded-memory encoding, online
streaming, adaptive compression parameters, and direct bit, sample, and range
access without decompressing the complete file.

Titchy is designed for integer samples such as ADC readings, IMU axes, counters,
and packed sensor registers. Timestamps should be stored separately or reconstructed 
from the sampling schedule.

## Features

- Lossless base/deviation transformation
- Dictionary-based base deduplication
- Adaptive base-ID and deviation widths
- Split-based independently decodable storage
- Bounded active dictionary with LRU eviction
- Batch and online packet encoding
- Indexed bit, sample, and range retrieval
- Raw little- and big-endian sensor byte support
- Deterministic demo, benchmarks, and parameter sweeps

## Quickstart

Install a current stable Rust toolchain, clone or download this repository, and
run:

```console
cargo run --release -- demo
```

The demo generates deterministic 16-bit sensor data and reports compression
ratio, encode/decode throughput, lossless verification, random access, range
access, bit access, and memory-limited operation.

Build and install the CLI from the checkout:

```console
cargo build --release
cargo install --path .
```

After installation, use `titchy` instead of `cargo run --release --` in the
examples below.

## CLI

Text input is whitespace-delimited unsigned integer samples:

```console
cargo run --release -- compress --bits 16 samples.txt samples.tchy
cargo run --release -- decompress samples.tchy restored.txt
cargo run --release -- get samples.tchy 42
cargo run --release -- range samples.tchy 100 16
```

Common compression options:

```text
--samples-per-chunk 4
--chunks-per-split 1000
--max-active-dictionary-bytes 1024
--endian little
```

For byte-aligned widths, preserve a binary sensor file's exact bit patterns:

```console
cargo run --release -- compress-raw \
  --bits 16 --endian little sensor.bin sensor.tchy
cargo run --release -- decompress-raw sensor.tchy restored.bin
```

Signed samples are supported through their fixed-width two's-complement bytes.

Online packet encoding and decoding:

```console
cargo run --release -- stream-encode \
  --bits 16 --chunks-per-packet 4 samples.txt samples.tstr
cargo run --release -- stream-decode samples.tstr restored.txt
```

## Library

```rust
use titchy_rs::{
    compress_samples, decompress_samples, get_by_sample_index, Result,
    TitchyConfig,
};

fn main() -> Result<()> {
    let samples = vec![1000, 1001, 1002, 1003, 1004];
    let config = TitchyConfig::new(16, 4, 1000)?;
    let compressed = compress_samples(&samples, config)?;
    let bytes = compressed.to_bytes()?;
    let decoded = decompress_samples(&compressed)?;
    let sample = get_by_sample_index(&compressed, 2)?;

    assert_eq!(decoded, samples);
    assert_eq!(sample, 1002);
    println!("{} compressed bytes", bytes.len());
    Ok(())
}
```

Runnable examples:

```console
cargo run --example basic
cargo run --example random_access
cargo run --example memory_limited
```

## Random Access

`IndexedTitchyReader` reads the header and split index once, then seeks only to
the pair and dictionary data needed for a request:

```rust
use std::{fs::File, io::BufReader};
use titchy_rs::{IndexedTitchyReader, Result};

fn read_values() -> Result<()> {
    let file = BufReader::new(File::open("samples.tchy")?);
    let mut reader = IndexedTitchyReader::open(file)?;

    let bit = reader.get_by_bit_index(8_015)?;
    let sample = reader.get_by_sample_index(500)?;
    let range = reader.get_range_by_sample_index(500, 32)?;
    println!("bit={bit}, sample={sample}, range={range:?}");
    Ok(())
}
```

The in-memory equivalents are `get_by_bit_index`, `get_by_sample_index`, and
`get_range_by_sample_index`.

## Best-Fit Data

Titchy works best when fixed-width samples repeat or share stable upper-bit
structure while lower bits contain sensor noise:

- IMU, accelerometer, gyroscope, and magnetometer axes
- ADC and environmental sensor readings
- Slowly changing counters and telemetry
- Fixed-rate signals where timestamps are implicit
- Packed signed or unsigned 8/12/16/24/32-bit values

High-entropy, encrypted, already-compressed, floating-point, or irregular
record-oriented data may expand. Floating-point streams can be supplied as raw
bit patterns, but their representation is not usually Titchy's best case.
Measure representative data before selecting production parameters.

## Benchmarks

Generate the deterministic benchmark report:

```console
cargo run --release -- benchmark \
  --samples 250000 --iterations 5 --access-operations 10000 \
  --markdown docs/benchmarks.md
```

Input-file benchmarks and parameter sweeps remain available:

```console
cargo run --release -- bench --bits 16 samples.txt
cargo run --release -- sweep --quick --bits 16 samples.txt
cargo bench --bench paper_bench
```

Local end-to-end comparison on a 10 MB deterministic 16-bit sensor stream:

| Codec | Serialized ratio | Encode MB/s | Decode MB/s |
| --- | ---: | ---: | ---: |
| Titchy | 0.501 | 9.87 | 24.05 |
| zstd 1.5.7, level 3 | 0.460 | 160.63 | 285.64 |
| LZ4 1.10.0, default | 0.735 | 306.15 | 295.52 |
| gzip 1.13, level 6 | 0.471 | 31.57 | 85.19 |

All decoded files were SHA-256 verified. Generic codecs are slower compared to
Titchy's native sample-level random access. The table uses one warmup and the 
median of five measured runs. Titchy-specific ratio and indexed latency results are in
[docs/benchmarks.md](docs/benchmarks.md).

## Development

On Ubuntu or Debian, install the native build tools and Rust:

```sh
sudo apt update
sudo apt install -y build-essential curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Run the repository-relative verification script from any directory:

```sh
sh /path/to/Titchy-rs/scripts/check.sh
```

Regenerate the deterministic benchmark report:

```sh
sh /path/to/Titchy-rs/scripts/benchmark.sh
```

Equivalent Cargo commands:

```console
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo run --release -- demo
```

See [docs/file-format.md](docs/file-format.md) for the binary layout.

## Paper Attribution

This project implements the compression algorithm from:

Rasmus Vestergaard, Qi Zhang, Marton Sipos, and Daniel E. Lucani, “Titchy:
Online Time-Series Compression With Random Access for the Internet of Things,”
*IEEE Internet of Things Journal*, vol. 8, no. 24, 2021.
[DOI: 10.1109/JIOT.2021.3081868](https://doi.org/10.1109/JIOT.2021.3081868).
