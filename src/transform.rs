use crate::{
    error::{Error, Result},
    params::TitchyConfig,
};

/// The base and deviation produced from one fixed-size chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformedChunk {
    /// Original samples with every deviation position cleared.
    pub base_samples: Vec<u64>,
    /// Extracted bits in Titchy's least-significant-level ordering.
    pub deviation_bits: Vec<u8>,
}

/// Splits a chunk into a deduplicable base and `l_d` deviation bits.
pub fn transform_chunk(
    samples: &[u64],
    l_d: usize,
    config: &TitchyConfig,
) -> Result<TransformedChunk> {
    let expected = config.samples_per_chunk as usize;
    if samples.len() != expected {
        return Err(Error::InvalidChunkLen {
            expected,
            actual: samples.len(),
        });
    }
    if l_d > config.chunk_bit_len() {
        return Err(Error::InvalidConfig("l_d cannot exceed chunk bit length"));
    }

    let positions = deviation_positions(config, l_d);
    let mut base_samples = samples.to_vec();
    let mut deviation_bits = Vec::with_capacity(l_d);

    for &(sample_idx, bit_pos) in &positions {
        let shift = config.bits_per_sample as usize - 1 - bit_pos;
        deviation_bits.push(((samples[sample_idx] >> shift) & 1) as u8);
        base_samples[sample_idx] &= !(1u64 << shift);
    }

    Ok(TransformedChunk {
        base_samples,
        deviation_bits,
    })
}

/// Restores a chunk by writing deviation bits into their base positions.
pub fn inverse_transform_chunk(
    base_samples: &[u64],
    deviation_bits: &[u8],
    l_d: usize,
    config: &TitchyConfig,
) -> Result<Vec<u64>> {
    let expected = config.samples_per_chunk as usize;
    if base_samples.len() != expected {
        return Err(Error::InvalidChunkLen {
            expected,
            actual: base_samples.len(),
        });
    }
    if deviation_bits.len() != l_d {
        return Err(Error::InvalidDeviationLen {
            expected: l_d,
            actual: deviation_bits.len(),
        });
    }

    let positions = deviation_positions(config, l_d);
    let mut samples = base_samples.to_vec();
    for ((sample_idx, bit_pos), bit) in positions.into_iter().zip(deviation_bits.iter().copied()) {
        let shift = config.bits_per_sample as usize - 1 - bit_pos;
        if bit & 1 == 1 {
            samples[sample_idx] |= 1u64 << shift;
        } else {
            samples[sample_idx] &= !(1u64 << shift);
        }
    }
    Ok(samples)
}

fn deviation_positions(config: &TitchyConfig, l_d: usize) -> Vec<(usize, usize)> {
    let c = config.samples_per_chunk as usize;
    let bits = config.bits_per_sample as usize;
    let base_count = l_d / c;
    let extra = l_d % c;
    let mut per_sample = vec![base_count; c];
    if extra > 0 {
        for count in per_sample.iter_mut().skip(c.saturating_sub(extra)) {
            *count += 1;
        }
    }

    let mut positions = Vec::with_capacity(l_d);
    for bit_pos in (0..bits).rev() {
        for (sample_idx, count) in per_sample.iter().copied().enumerate() {
            if bits - bit_pos <= count {
                positions.push((sample_idx, bit_pos));
            }
        }
    }
    positions
}

pub(crate) fn is_deviation_position(
    config: &TitchyConfig,
    l_d: usize,
    sample_index: usize,
    bit_position: usize,
) -> bool {
    deviation_positions(config, l_d)
        .into_iter()
        .any(|position| position == (sample_index, bit_position))
}

pub(crate) fn deviation_bit_offset(
    config: &TitchyConfig,
    l_d: usize,
    sample_index: usize,
    bit_position: usize,
) -> Option<usize> {
    deviation_positions(config, l_d)
        .into_iter()
        .position(|position| position == (sample_index, bit_position))
}
