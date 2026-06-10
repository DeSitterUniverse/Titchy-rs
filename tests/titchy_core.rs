use titchy_rs::{
    compress_samples, decompress_samples, get_by_bit_index, get_by_sample_index,
    get_range_by_sample_index, inverse_transform_chunk, transform_chunk, SampleEndian,
    TitchyConfig,
};

#[test]
fn config_supports_paper_c16_and_rejects_unrepresentable_initial_deviation() {
    assert!(TitchyConfig::new(16, 16, 1000).is_ok());
    assert!(TitchyConfig::new(64, 7, 1000).is_ok());
    assert!(TitchyConfig::new(64, 8, 1000).is_err());
}

#[test]
fn transform_extracts_lsb_evenly_across_samples() {
    let cfg = TitchyConfig::new(4, 3, 8)
        .unwrap()
        .with_endian(SampleEndian::Big);
    let chunk = [0b1011, 0b0101, 0b1110];

    let transformed = transform_chunk(&chunk, 5, &cfg).unwrap();

    assert_eq!(transformed.deviation_bits, vec![1, 1, 0, 0, 1]);
    assert_eq!(transformed.base_samples, vec![0b1010, 0b0100, 0b1100]);
    assert_eq!(
        inverse_transform_chunk(
            &transformed.base_samples,
            &transformed.deviation_bits,
            5,
            &cfg
        )
        .unwrap(),
        chunk
    );
}

#[test]
fn round_trip_raw_fixed_width_samples() {
    let cfg = TitchyConfig::new(16, 4, 5).unwrap();
    let mut samples = Vec::new();
    for _ in 0..20 {
        samples.extend([1000, 1001, 1002, 1003]);
    }

    let compressed = compress_samples(&samples, cfg).unwrap();
    let decoded = decompress_samples(&compressed).unwrap();

    assert_eq!(decoded, samples);
    assert!(compressed.compressed_bit_len() < samples.len() * 16);
}

#[test]
fn final_partial_chunk_is_zero_padded_then_truncated() {
    let cfg = TitchyConfig::new(8, 4, 3).unwrap();
    let samples = vec![9, 8, 7, 6, 5, 4];

    let compressed = compress_samples(&samples, cfg).unwrap();

    assert_eq!(compressed.original_sample_count(), 6);
    assert_eq!(decompress_samples(&compressed).unwrap(), samples);
}

#[test]
fn random_access_matches_full_decompression_across_split_boundary() {
    let cfg = TitchyConfig::new(12, 3, 2).unwrap();
    let samples = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let compressed = compress_samples(&samples, cfg).unwrap();
    let decoded = decompress_samples(&compressed).unwrap();

    for (i, expected) in decoded.iter().enumerate().take(samples.len()) {
        assert_eq!(get_by_sample_index(&compressed, i).unwrap(), *expected);
    }

    assert_eq!(
        get_range_by_sample_index(&compressed, 2, 6).unwrap(),
        decoded[2..8]
    );
}

#[test]
fn random_access_range_rejects_length_overflow() {
    let compressed = compress_samples(&[1, 2, 3], TitchyConfig::new(8, 1, 2).unwrap()).unwrap();

    assert!(get_range_by_sample_index(&compressed, 1, usize::MAX).is_err());
}

#[test]
fn bit_random_access_matches_msb_first_sample_bits() {
    let config = TitchyConfig::new(4, 3, 2).unwrap();
    let samples = vec![0b1011, 0b0101, 0b1110, 0b0011];
    let compressed = compress_samples(&samples, config).unwrap();
    let expected = [
        1, 0, 1, 1, //
        0, 1, 0, 1, //
        1, 1, 1, 0, //
        0, 0, 1, 1,
    ];

    for (bit_index, expected_bit) in expected.into_iter().enumerate() {
        assert_eq!(
            get_by_bit_index(&compressed, bit_index).unwrap(),
            expected_bit
        );
    }

    assert!(get_by_bit_index(&compressed, expected.len()).is_err());
}

#[test]
fn lru_memory_cap_does_not_break_decode() {
    let cfg = TitchyConfig::new(8, 2, 3)
        .unwrap()
        .with_max_active_dictionary_bytes(Some(2));
    let samples = vec![10, 11, 20, 21, 30, 31, 10, 11, 40, 41, 20, 21];

    let compressed = compress_samples(&samples, cfg).unwrap();

    assert!(compressed.dictionary_len() > compressed.max_active_dictionary_bases().unwrap());
    assert_eq!(decompress_samples(&compressed).unwrap(), samples);
}

#[test]
fn evicted_base_is_reintroduced_with_a_new_persisted_id() {
    let cfg = TitchyConfig::new(8, 1, 100)
        .unwrap()
        .with_max_active_dictionary_bytes(Some(1));
    let samples = vec![0x10, 0x20, 0x10];

    let compressed = compress_samples(&samples, cfg).unwrap();

    assert_eq!(compressed.dictionary_len(), 3);
    assert_eq!(decompress_samples(&compressed).unwrap(), samples);
}

#[test]
fn active_dictionary_capacity_counts_base_and_id_bytes() {
    let cfg = TitchyConfig::new(16, 1, 100)
        .unwrap()
        .with_max_active_dictionary_bytes(Some(12));
    let compressed = compress_samples(&[1, 2, 3], cfg).unwrap();

    assert_eq!(compressed.max_active_dictionary_bases(), Some(2));
}

#[test]
fn split_parameters_adapt_from_global_persisted_dictionary_growth() {
    let cfg = TitchyConfig::new(4, 1, 2).unwrap();
    let samples = vec![0, 4, 8, 12];

    let compressed = compress_samples(&samples, cfg).unwrap();
    let splits = compressed.split_metadata();

    assert_eq!(splits.len(), 2);
    assert_eq!((splits[0].l_id, splits[0].l_d), (1, 2));
    assert_eq!((splits[1].l_id, splits[1].l_d), (2, 3));
}

#[test]
fn binary_container_round_trips_and_preserves_random_access() {
    let cfg = TitchyConfig::new(10, 3, 4).unwrap();
    let samples = vec![0, 1, 2, 3, 4, 5, 511, 512, 513, 7, 8, 9, 10];
    let compressed = compress_samples(&samples, cfg).unwrap();

    let encoded = compressed.to_bytes().unwrap();
    let decoded_blob = titchy_rs::CompressedTitchy::from_bytes(&encoded).unwrap();

    assert_eq!(decompress_samples(&decoded_blob).unwrap(), samples);
    assert_eq!(get_by_sample_index(&decoded_blob, 8).unwrap(), 513);
}
