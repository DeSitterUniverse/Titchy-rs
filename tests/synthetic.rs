use titchy_rs::synthetic_sensor_samples;

#[test]
fn synthetic_sensor_data_is_deterministic_and_width_bounded() {
    let first = synthetic_sensor_samples(128);
    let second = synthetic_sensor_samples(128);

    assert_eq!(first, second);
    assert_eq!(first.len(), 128);
    assert!(first.iter().all(|sample| *sample <= u16::MAX as u64));
    assert!(first.windows(2).any(|pair| pair[0] != pair[1]));
}
