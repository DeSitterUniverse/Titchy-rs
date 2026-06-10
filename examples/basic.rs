use titchy_rs::{
    compress_samples, decompress_samples, synthetic_sensor_samples, Result, TitchyConfig,
};

fn main() -> Result<()> {
    let samples = synthetic_sensor_samples(10_000);
    let config = TitchyConfig::new(16, 4, 1000)?;
    let compressed = compress_samples(&samples, config)?;
    let encoded = compressed.to_bytes()?;
    let restored = titchy_rs::CompressedTitchy::from_bytes(&encoded)?;
    let decoded = decompress_samples(&restored)?;

    assert_eq!(decoded, samples);
    println!("samples: {}", samples.len());
    println!("encoded bytes: {}", encoded.len());
    println!(
        "serialized ratio: {:.4}",
        encoded.len() as f64 / (samples.len() * 2) as f64
    );
    println!("lossless verification: passed");
    Ok(())
}
