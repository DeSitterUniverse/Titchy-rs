use crate::{
    codec::{decompress_samples, CompressedTitchy},
    error::{Error, Result},
    transform::deviation_bit_offset,
};

/// Retrieves one MSB-first bit from an in-memory compressed container.
pub fn get_by_bit_index(compressed: &CompressedTitchy, bit_index: usize) -> Result<u8> {
    let config = compressed.config();
    let bit_len = compressed
        .original_sample_count()
        .checked_mul(config.bits_per_sample as usize)
        .ok_or(Error::InvalidFormat("uncompressed bit length overflow"))?;
    if bit_index >= bit_len {
        return Err(Error::BitIndexOutOfRange {
            index: bit_index,
            len: bit_len,
        });
    }

    let bits_per_sample = config.bits_per_sample as usize;
    let sample_index = bit_index / bits_per_sample;
    let bit_in_sample = bit_index % bits_per_sample;
    let chunk_index = sample_index / config.samples_per_chunk as usize;
    let sample_in_chunk = sample_index % config.samples_per_chunk as usize;
    let (split, pair_index) = compressed
        .split_for_chunk(chunk_index)
        .ok_or(Error::InvalidFormat("sample chunk is missing"))?;
    let pair = &split.pairs[pair_index];

    if let Some(offset) =
        deviation_bit_offset(config, split.l_d as usize, sample_in_chunk, bit_in_sample)
    {
        return pair
            .deviation_bits
            .get(offset)
            .copied()
            .ok_or(Error::InvalidFormat("deviation bit is missing"));
    }

    let base = compressed
        .dictionary
        .get(pair.base_id as usize)
        .ok_or(Error::DictionaryIdOutOfRange(pair.base_id))?;
    Ok(base.get_bit(sample_in_chunk * bits_per_sample + bit_in_sample))
}

/// Retrieves one sample by reconstructing only its containing chunk.
pub fn get_by_sample_index(compressed: &CompressedTitchy, index: usize) -> Result<u64> {
    if index >= compressed.original_sample_count() {
        return Err(Error::SampleIndexOutOfRange {
            index,
            len: compressed.original_sample_count(),
        });
    }

    let config = compressed.config();
    let chunk_index = index / config.samples_per_chunk as usize;
    let sample_in_chunk = index % config.samples_per_chunk as usize;
    let (split, pair_index) =
        compressed
            .split_for_chunk(chunk_index)
            .ok_or(Error::SampleIndexOutOfRange {
                index,
                len: compressed.original_sample_count(),
            })?;
    let chunk = compressed.reconstruct_chunk(&split.pairs[pair_index], split.l_d as usize)?;
    Ok(chunk[sample_in_chunk])
}

/// Retrieves a contiguous range of samples.
pub fn get_range_by_sample_index(
    compressed: &CompressedTitchy,
    start: usize,
    len: usize,
) -> Result<Vec<u64>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let end = start.checked_add(len).ok_or(Error::SampleIndexOutOfRange {
        index: usize::MAX,
        len: compressed.original_sample_count(),
    })?;
    if end > compressed.original_sample_count() {
        return Err(Error::SampleIndexOutOfRange {
            index: end - 1,
            len: compressed.original_sample_count(),
        });
    }

    let mut out = Vec::with_capacity(len);
    for index in start..end {
        out.push(get_by_sample_index(compressed, index)?);
    }
    Ok(out)
}

#[allow(dead_code)]
fn get_range_by_full_decode(
    compressed: &CompressedTitchy,
    start: usize,
    len: usize,
) -> Result<Vec<u64>> {
    let samples = decompress_samples(compressed)?;
    Ok(samples[start..start + len].to_vec())
}
