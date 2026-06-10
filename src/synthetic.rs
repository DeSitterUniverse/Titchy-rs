/// Generates deterministic 16-bit sensor-like samples.
///
/// The sequence combines a slowly moving value, bounded pseudo-random noise,
/// and a periodic drift. It is stable across platforms and process runs, which
/// makes it suitable for examples, demos, and reproducible benchmarks.
pub fn synthetic_sensor_samples(count: usize) -> Vec<u64> {
    let mut state = 0x1234_5678u32;
    let mut value = 32_768i32;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = ((state >> 29) as i32) - 3;
        let periodic = ((index / 1024) % 17) as i32 - 8;
        value = (value + noise + periodic.signum()).clamp(0, u16::MAX as i32);
        samples.push(value as u64);
    }
    samples
}
