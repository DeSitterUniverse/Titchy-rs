use titchy_rs::{
    bit_access_cost, compress_samples, expected_block_count, sample_access_cost,
    titchy_access_bounds, universal_access_cost, TitchyConfig,
};

#[test]
fn expected_block_count_matches_paper_lemma_one() {
    assert_eq!(expected_block_count(1, 64).unwrap(), 1.0);
    assert_eq!(expected_block_count(64, 64).unwrap(), 1.984375);
    assert_eq!(expected_block_count(65, 64).unwrap(), 2.0);
}

#[test]
fn universal_cost_matches_theorem_one() {
    let cost = universal_access_cost(64, 1024, 0.5).unwrap();

    assert_eq!(cost, 543.5);
}

#[test]
fn bit_and_sample_costs_follow_equations_fourteen_and_fifteen() {
    let config = TitchyConfig::new(8, 2, 4).unwrap();
    let compressed = compress_samples(&[0xab, 0xcd], config).unwrap();

    assert_eq!(bit_access_cost(&compressed, 7).unwrap(), 17);
    assert_eq!(bit_access_cost(&compressed, 0).unwrap(), 19);
    assert_eq!(sample_access_cost(&compressed, 0).unwrap(), 26);
}

#[test]
fn arbitrary_range_bounds_include_parameters_pairs_and_bases() {
    let config = TitchyConfig::new(8, 2, 2).unwrap();
    let compressed = compress_samples(&[1, 2, 1, 2, 3, 4], config).unwrap();

    let bounds = titchy_access_bounds(&compressed, 0, 32).unwrap();

    assert_eq!(bounds.chunk_count, 2);
    assert_eq!(bounds.split_count, 1);
    assert!(bounds.best_case_bits <= bounds.worst_case_bits);
    assert!(bounds.best_case_bits >= 16 + 2 * (1 + 8));
}
