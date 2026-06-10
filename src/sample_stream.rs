use crate::{
    error::{Error, Result},
    packed_bits::PackedBits,
    params::TitchyConfig,
};

pub fn validate_sample(sample: u64, bits_per_sample: u8) -> Result<()> {
    if bits_per_sample < 64 && sample >= (1u64 << bits_per_sample) {
        return Err(Error::InvalidSample {
            sample,
            bits_per_sample,
        });
    }
    Ok(())
}

pub fn pack_samples(samples: &[u64], config: &TitchyConfig) -> Result<PackedBits> {
    let mut bits = Vec::with_capacity(samples.len() * config.bits_per_sample as usize);
    for &sample in samples {
        validate_sample(sample, config.bits_per_sample)?;
        for bit_pos in 0..config.bits_per_sample {
            let shift = config.bits_per_sample - 1 - bit_pos;
            bits.push(((sample >> shift) & 1) as u8);
        }
    }
    Ok(PackedBits::from_bits(&bits))
}

pub fn unpack_samples(bits: &PackedBits, config: &TitchyConfig) -> Vec<u64> {
    let sample_count = bits.bit_len() / config.bits_per_sample as usize;
    let mut samples = Vec::with_capacity(sample_count);
    for sample_idx in 0..sample_count {
        let mut value = 0u64;
        for bit_pos in 0..config.bits_per_sample {
            let packed_index = sample_idx * config.bits_per_sample as usize + bit_pos as usize;
            value = (value << 1) | bits.get_bit(packed_index) as u64;
        }
        samples.push(value);
    }
    samples
}

pub fn chunk_samples(samples: &[u64], config: &TitchyConfig) -> Result<Vec<Vec<u64>>> {
    let c = config.samples_per_chunk as usize;
    let mut chunks = Vec::new();
    for source in samples.chunks(c) {
        let mut chunk = source.to_vec();
        while chunk.len() < c {
            chunk.push(0);
        }
        for &sample in &chunk {
            validate_sample(sample, config.bits_per_sample)?;
        }
        chunks.push(chunk);
    }
    Ok(chunks)
}
