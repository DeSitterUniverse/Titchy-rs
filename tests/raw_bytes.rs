use titchy_rs::{compress_raw_bytes, decompress_raw_bytes, SampleEndian, TitchyConfig};

#[test]
fn little_endian_raw_samples_round_trip() {
    let config = TitchyConfig::new(16, 2, 3)
        .unwrap()
        .with_endian(SampleEndian::Little);
    let raw = vec![0x34, 0x12, 0xcd, 0xab, 0x02, 0x01];

    let compressed = compress_raw_bytes(&raw, config).unwrap();

    assert_eq!(decompress_raw_bytes(&compressed).unwrap(), raw);
}

#[test]
fn big_endian_raw_samples_round_trip() {
    let config = TitchyConfig::new(16, 2, 3)
        .unwrap()
        .with_endian(SampleEndian::Big);
    let raw = vec![0x12, 0x34, 0xab, 0xcd, 0x01, 0x02];

    let compressed = compress_raw_bytes(&raw, config).unwrap();

    assert_eq!(decompress_raw_bytes(&compressed).unwrap(), raw);
}

#[test]
fn raw_byte_api_rejects_non_byte_aligned_sample_widths() {
    let config = TitchyConfig::new(12, 2, 3).unwrap();

    assert!(compress_raw_bytes(&[0, 1, 2], config).is_err());
}
