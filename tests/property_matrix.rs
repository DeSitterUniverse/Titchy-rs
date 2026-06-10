use titchy_rs::{
    compress_samples, decompress_samples, get_by_sample_index, CompressedTitchy, TitchyConfig,
};

#[test]
fn deterministic_width_and_chunk_matrix_round_trips() {
    for bits in 1u8..=16 {
        let max_chunk = (u8::MAX as usize / bits as usize).min(8);
        let mask = (1u64 << bits) - 1;
        for samples_per_chunk in 1..=max_chunk as u8 {
            let samples = (0..37)
                .map(|index| ((index as u64 * 73) ^ (index as u64 * index as u64 * 11)) & mask)
                .collect::<Vec<_>>();
            let config = TitchyConfig::new(bits, samples_per_chunk, 3).unwrap();
            let compressed = compress_samples(&samples, config).unwrap();
            let bytes = compressed.to_bytes().unwrap();
            let restored = CompressedTitchy::from_bytes(&bytes).unwrap();

            assert_eq!(decompress_samples(&restored).unwrap(), samples);
            for (index, expected) in samples.iter().copied().enumerate() {
                assert_eq!(get_by_sample_index(&restored, index).unwrap(), expected);
            }
        }
    }
}
