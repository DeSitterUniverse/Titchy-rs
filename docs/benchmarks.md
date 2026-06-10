# Titchy Benchmark Results

Deterministic 16-bit sensor data, 250000 samples, 5 timing iterations, and 10000 indexed access operations per latency measurement.

Command: `cargo run --release -- benchmark --samples 250000 --iterations 5 --access-operations 10000`

| Dictionary | Paper ratio | Serialized ratio | Encode MB/s | Decode MB/s | Sample access ns | Range access ns |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| unlimited | 0.6607 | 0.6645 | 17.75 | 26.89 | 480 | 2322 |
| 1 KiB | 0.6712 | 0.6751 | 18.95 | 27.12 | 487 | 2342 |

Ratios are compressed size divided by the original fixed-width sample size; lower is better. Range latency retrieves 16 samples. Timing results are machine-dependent.
