use std::io::Cursor;

use titchy_rs::{
    compress_samples, synthetic_sensor_samples, IndexedTitchyReader, Result, TitchyConfig,
};

fn main() -> Result<()> {
    let samples = synthetic_sensor_samples(10_000);
    let compressed = compress_samples(&samples, TitchyConfig::new(16, 4, 1000)?)?;
    let mut reader = IndexedTitchyReader::open(Cursor::new(compressed.to_bytes()?))?;

    let sample = reader.get_by_sample_index(5_000)?;
    let range = reader.get_range_by_sample_index(5_000, 8)?;
    let least_significant_bit = reader.get_by_bit_index(5_000 * 16 + 15)?;

    assert_eq!(sample, samples[5_000]);
    assert_eq!(range, samples[5_000..5_008]);
    assert_eq!(least_significant_bit, (samples[5_000] & 1) as u8);
    println!("sample[5000] = {sample}");
    println!("samples[5000..5008] = {range:?}");
    println!("sample[5000] least-significant bit = {least_significant_bit}");
    Ok(())
}
