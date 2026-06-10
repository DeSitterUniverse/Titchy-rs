use titchy_rs::{
    compress_samples, decompress_samples, synthetic_sensor_samples, Result, TitchyConfig,
};

fn main() -> Result<()> {
    let samples = synthetic_sensor_samples(10_000);
    let config = TitchyConfig::new(16, 4, 1000)?.with_max_active_dictionary_bytes(Some(1024));
    let compressed = compress_samples(&samples, config)?;
    let decoded = decompress_samples(&compressed)?;

    assert_eq!(decoded, samples);
    println!("active dictionary limit: 1024 bytes");
    println!(
        "maximum active bases: {}",
        compressed
            .max_active_dictionary_bases()
            .expect("memory limit is configured")
    );
    println!("persisted bases: {}", compressed.dictionary_len());
    println!("lossless verification: passed");
    Ok(())
}
