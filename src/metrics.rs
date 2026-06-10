use crate::{
    codec::CompressedTitchy,
    error::{Error, Result},
    transform::is_deviation_position,
};

/// Best- and worst-case compressed-bit access bounds for a requested range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessCostBounds {
    /// Best-case number of compressed bits read.
    pub best_case_bits: usize,
    /// Worst-case number of compressed bits read.
    pub worst_case_bits: usize,
    /// Number of chunks intersecting the request.
    pub chunk_count: usize,
    /// Number of splits intersecting the request.
    pub split_count: usize,
}

/// Computes the expected number of fixed-size blocks touched by a request.
pub fn expected_block_count(requested_bits: usize, block_bits: usize) -> Result<f64> {
    if requested_bits == 0 || block_bits == 0 {
        return Err(Error::InvalidConfig(
            "requested_bits and block_bits must be non-zero",
        ));
    }
    let ceiling = requested_bits.div_ceil(block_bits) as f64;
    let alignment = ((requested_bits - 1) % block_bits) as f64 / block_bits as f64;
    Ok(ceiling + alignment)
}

/// Computes the paper's universal compressed-access cost.
pub fn universal_access_cost(
    requested_bits: usize,
    block_bits: usize,
    compression_ratio: f64,
) -> Result<f64> {
    if !compression_ratio.is_finite() || compression_ratio < 0.0 {
        return Err(Error::InvalidConfig(
            "compression_ratio must be finite and non-negative",
        ));
    }
    Ok(compression_ratio * block_bits as f64 * expected_block_count(requested_bits, block_bits)?)
}

/// Computes Titchy's compressed-bit cost for one original bit.
pub fn bit_access_cost(compressed: &CompressedTitchy, bit_index: usize) -> Result<usize> {
    let config = compressed.config();
    let total_bits = compressed.original_sample_count() * config.bits_per_sample as usize;
    if bit_index >= total_bits {
        return Err(Error::SampleIndexOutOfRange {
            index: bit_index,
            len: total_bits,
        });
    }

    let chunk_bits = config.chunk_bit_len();
    let chunk_index = bit_index / chunk_bits;
    let bit_in_chunk = bit_index % chunk_bits;
    let sample_in_chunk = bit_in_chunk / config.bits_per_sample as usize;
    let bit_position = bit_in_chunk % config.bits_per_sample as usize;
    let (split, _) = compressed
        .split_for_chunk(chunk_index)
        .ok_or(Error::InvalidFormat("bit is not covered by a split"))?;
    let in_deviation =
        is_deviation_position(config, split.l_d as usize, sample_in_chunk, bit_position);
    Ok(16 + usize::from(!in_deviation) * split.l_id as usize + 1)
}

/// Computes Titchy's compressed-bit cost for one original sample.
pub fn sample_access_cost(compressed: &CompressedTitchy, sample_index: usize) -> Result<usize> {
    if sample_index >= compressed.original_sample_count() {
        return Err(Error::SampleIndexOutOfRange {
            index: sample_index,
            len: compressed.original_sample_count(),
        });
    }
    let config = compressed.config();
    let chunk_index = sample_index / config.samples_per_chunk as usize;
    let (split, _) = compressed
        .split_for_chunk(chunk_index)
        .ok_or(Error::InvalidFormat("sample is not covered by a split"))?;
    Ok(16 + split.l_id as usize + config.bits_per_sample as usize)
}

/// Computes best- and worst-case access bounds for an arbitrary bit range.
pub fn titchy_access_bounds(
    compressed: &CompressedTitchy,
    start_bit: usize,
    requested_bits: usize,
) -> Result<AccessCostBounds> {
    if requested_bits == 0 {
        return Err(Error::InvalidConfig("requested_bits must be non-zero"));
    }
    let config = compressed.config();
    let total_bits = compressed.original_sample_count() * config.bits_per_sample as usize;
    let end_bit = start_bit
        .checked_add(requested_bits)
        .ok_or(Error::InvalidFormat("requested range overflows"))?;
    if end_bit > total_bits {
        return Err(Error::SampleIndexOutOfRange {
            index: end_bit - 1,
            len: total_bits,
        });
    }

    let chunk_bits = config.chunk_bit_len();
    let first_chunk = start_bit / chunk_bits;
    let last_chunk = (end_bit - 1) / chunk_bits;
    let chunk_count = last_chunk - first_chunk + 1;
    let mut split_count = 0usize;
    let mut pair_and_parameter_bits = 0usize;
    let mut split_chunk_start = 0usize;

    for split in &compressed.splits {
        let split_chunk_end = split_chunk_start + split.pairs.len();
        let overlap_start = first_chunk.max(split_chunk_start);
        let overlap_end = (last_chunk + 1).min(split_chunk_end);
        if overlap_start < overlap_end {
            let overlap = overlap_end - overlap_start;
            split_count += 1;
            pair_and_parameter_bits += 16 + overlap * (split.l_id as usize + split.l_d as usize);
        }
        split_chunk_start = split_chunk_end;
    }

    let best_case_bits = chunk_bits + pair_and_parameter_bits;
    let worst_case_bits =
        chunk_bits * chunk_count.min(compressed.dictionary.len()) + pair_and_parameter_bits;
    Ok(AccessCostBounds {
        best_case_bits,
        worst_case_bits,
        chunk_count,
        split_count,
    })
}
