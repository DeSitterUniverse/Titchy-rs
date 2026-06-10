use std::io::Cursor;

use titchy_rs::{compress_samples, decompress_samples, IndexedTitchyReader, TitchyConfig};

#[test]
fn current_container_has_index_and_round_trips() {
    let config = TitchyConfig::new(12, 3, 2).unwrap();
    let samples = vec![10, 11, 12, 20, 21, 22, 30, 31, 32, 40];
    let compressed = compress_samples(&samples, config).unwrap();

    let bytes = compressed.to_bytes().unwrap();

    assert_eq!(&bytes[..4], b"TCHY");
    assert_eq!(bytes[4], 2);
    assert_eq!(
        decompress_samples(&titchy_rs::CompressedTitchy::from_bytes(&bytes).unwrap()).unwrap(),
        samples
    );
}

#[test]
fn indexed_reader_retrieves_samples_and_ranges_without_deserializing_container() {
    let config = TitchyConfig::new(16, 4, 3).unwrap();
    let samples = (0..37).map(|value| value * 17).collect::<Vec<_>>();
    let compressed = compress_samples(&samples, config).unwrap();
    let bytes = compressed.to_bytes().unwrap();
    let mut reader = IndexedTitchyReader::open(Cursor::new(bytes)).unwrap();

    assert_eq!(reader.get_by_sample_index(0).unwrap(), samples[0]);
    assert_eq!(reader.get_by_sample_index(19).unwrap(), samples[19]);
    assert_eq!(reader.get_by_sample_index(36).unwrap(), samples[36]);
    assert_eq!(
        reader.get_range_by_sample_index(7, 18).unwrap(),
        samples[7..25]
    );
}

#[test]
fn indexed_reader_retrieves_individual_bits() {
    let config = TitchyConfig::new(5, 3, 2).unwrap();
    let samples = vec![0b10101, 0b00111, 0b11100, 0b01010];
    let compressed = compress_samples(&samples, config).unwrap();
    let bytes = compressed.to_bytes().unwrap();
    let mut reader = IndexedTitchyReader::open(Cursor::new(bytes)).unwrap();

    for bit_index in 0..samples.len() * 5 {
        let sample = samples[bit_index / 5];
        let shift = 4 - bit_index % 5;
        assert_eq!(
            reader.get_by_bit_index(bit_index).unwrap(),
            ((sample >> shift) & 1) as u8
        );
    }
}

#[test]
fn readers_reject_an_unsupported_format_marker() {
    let config = TitchyConfig::new(8, 2, 3).unwrap();
    let mut bytes = compress_samples(&[1, 2, 3, 4], config)
        .unwrap()
        .to_bytes()
        .unwrap();
    bytes[4] = 0;

    assert!(titchy_rs::CompressedTitchy::from_bytes(&bytes).is_err());
    assert!(IndexedTitchyReader::open(Cursor::new(bytes)).is_err());
}
