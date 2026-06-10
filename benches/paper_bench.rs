use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use titchy_rs::{compress_samples, decompress_samples, synthetic_sensor_samples, TitchyConfig};

fn main() {
    let samples = synthetic_sensor_samples(250_000);
    let uncompressed_bytes = samples.len() as f64 * 2.0;
    println!(
        "samples_per_chunk,chunks_per_split,dictionary_cap_bytes,paper_ratio,encode_mb_s,decode_mb_s"
    );

    for samples_per_chunk in [1u8, 2, 4, 8, 16] {
        for chunks_per_split in [100u16, 300, 1000, 3000] {
            for dictionary_cap in [None, Some(1024), Some(10 * 1024)] {
                let config = TitchyConfig::new(16, samples_per_chunk, chunks_per_split)
                    .unwrap()
                    .with_max_active_dictionary_bytes(dictionary_cap);
                let (compressed, encode_time) =
                    timed(|| compress_samples(black_box(&samples), config.clone()).unwrap());
                let (decoded, decode_time) =
                    timed(|| decompress_samples(black_box(&compressed)).unwrap());
                assert_eq!(decoded, samples);

                let ratio =
                    compressed.paper_compressed_bit_len() as f64 / (samples.len() * 16) as f64;
                println!(
                    "{samples_per_chunk},{chunks_per_split},{},{ratio:.6},{:.3},{:.3}",
                    dictionary_cap
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unlimited".to_string()),
                    throughput_mb_s(uncompressed_bytes, encode_time),
                    throughput_mb_s(uncompressed_bytes, decode_time),
                );
            }
        }
    }
}

fn timed<T>(operation: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let result = operation();
    (result, start.elapsed())
}

fn throughput_mb_s(bytes: f64, elapsed: Duration) -> f64 {
    bytes / elapsed.as_secs_f64().max(1e-9) / 1_000_000.0
}
