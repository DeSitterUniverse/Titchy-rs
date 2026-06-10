use crate::{
    bitio::{BitReader, BitWriter},
    dictionary::Dictionary,
    error::{Error, Result},
    packed_bits::PackedBits,
    params::TitchyConfig,
    sample_stream::{chunk_samples, pack_samples, unpack_samples},
    transform::{inverse_transform_chunk, transform_chunk},
};

pub(crate) const FORMAT_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedPair {
    pub base_id: u32,
    pub deviation_bits: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub l_id: u8,
    pub l_d: u8,
    pub pairs: Vec<EncodedPair>,
}

/// Read-only metadata describing one compressed split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitMetadata {
    /// Number of bits used by each base ID.
    pub l_id: u8,
    /// Number of deviation bits stored for each chunk.
    pub l_d: u8,
    /// Number of chunks in the split.
    pub chunk_count: usize,
}

/// An in-memory Titchy container.
///
/// Use [`CompressedTitchy::to_bytes`] to serialize it and
/// [`CompressedTitchy::from_bytes`] to parse the current indexed format.
#[derive(Debug, Clone)]
pub struct CompressedTitchy {
    pub(crate) config: TitchyConfig,
    pub(crate) original_sample_count: usize,
    pub(crate) dictionary: Vec<PackedBits>,
    pub(crate) splits: Vec<Split>,
    pub(crate) max_active_dictionary_bases: Option<usize>,
}

impl CompressedTitchy {
    /// Returns the number of original samples before final-chunk padding.
    pub fn original_sample_count(&self) -> usize {
        self.original_sample_count
    }

    /// Returns the number of persisted dictionary bases.
    pub fn dictionary_len(&self) -> usize {
        self.dictionary.len()
    }

    /// Returns the configured maximum number of active encoder bases.
    pub fn max_active_dictionary_bases(&self) -> Option<usize> {
        self.max_active_dictionary_bases
    }

    /// Returns the adaptive parameters and chunk count for every split.
    pub fn split_metadata(&self) -> Vec<SplitMetadata> {
        self.splits
            .iter()
            .map(|split| SplitMetadata {
                l_id: split.l_id,
                l_d: split.l_d,
                chunk_count: split.pairs.len(),
            })
            .collect()
    }

    /// Returns the paper-model compressed size in bits.
    pub fn compressed_bit_len(&self) -> usize {
        self.paper_compressed_bit_len()
    }

    /// Returns the dictionary, pair, and split-parameter size used by the
    /// paper's compression-ratio accounting.
    pub fn paper_compressed_bit_len(&self) -> usize {
        let dictionary_bits = self.dictionary.len() * self.config.chunk_bit_len();
        let split_bits: usize = self
            .splits
            .iter()
            .map(|split| 16 + split.pairs.len() * (split.l_id as usize + split.l_d as usize))
            .sum();
        dictionary_bits + split_bits
    }

    /// Returns the complete serialized container size in bits.
    pub fn serialized_bit_len(&self) -> Result<usize> {
        Ok(self.to_bytes()?.len() * 8)
    }

    /// Serializes the current indexed `TCHY` container.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        const MAGIC: &[u8; 4] = b"TCHY";
        const HEADER_LEN: usize = 38;
        const INDEX_ENTRY_LEN: usize = 24;
        let split_records = self
            .splits
            .iter()
            .map(encode_split)
            .collect::<Result<Vec<_>>>()?;
        let index_len = INDEX_ENTRY_LEN * self.splits.len();
        let dictionary_len = self.dictionary.len() * self.config.chunk_byte_len();
        let mut next_split_offset = HEADER_LEN + index_len + dictionary_len;

        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.push(FORMAT_VERSION);
        out.push(self.config.bits_per_sample);
        out.push(self.config.samples_per_chunk);
        out.push(match self.config.endian {
            crate::params::SampleEndian::Little => 0,
            crate::params::SampleEndian::Big => 1,
        });
        out.extend_from_slice(&self.config.chunks_per_split.to_le_bytes());
        let threshold_ppm = (self.config.new_base_threshold * 1_000_000.0).round() as u32;
        out.extend_from_slice(&threshold_ppm.to_le_bytes());
        let max_active = self
            .config
            .max_active_dictionary_bytes
            .map(|value| value as u64)
            .unwrap_or(u64::MAX);
        out.extend_from_slice(&max_active.to_le_bytes());
        out.extend_from_slice(&(self.original_sample_count as u64).to_le_bytes());
        out.extend_from_slice(&(self.dictionary.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.splits.len() as u32).to_le_bytes());

        let mut chunk_start = 0u64;
        for (split, record) in self.splits.iter().zip(&split_records) {
            out.extend_from_slice(&chunk_start.to_le_bytes());
            out.extend_from_slice(&(split.pairs.len() as u32).to_le_bytes());
            out.extend_from_slice(&(next_split_offset as u64).to_le_bytes());
            out.extend_from_slice(&(record.len() as u32).to_le_bytes());
            chunk_start += split.pairs.len() as u64;
            next_split_offset += record.len();
        }

        let base_byte_len = self.config.chunk_byte_len();
        for base in &self.dictionary {
            if base.bit_len() != self.config.chunk_bit_len() {
                return Err(Error::InvalidFormat("dictionary base has wrong bit length"));
            }
            out.extend_from_slice(base.bytes());
            let padding = base_byte_len.saturating_sub(base.bytes().len());
            out.extend(std::iter::repeat_n(0, padding));
        }

        for record in split_records {
            out.extend_from_slice(&record);
        }
        Ok(out)
    }

    /// Parses the current indexed `TCHY` container.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        const MAGIC: &[u8; 4] = b"TCHY";
        let mut cursor = ByteCursor::new(bytes);
        if cursor.read_exact(4)? != MAGIC {
            return Err(Error::InvalidFormat("missing TCHY magic"));
        }
        let version = cursor.read_u8()?;
        if version != FORMAT_VERSION {
            return Err(Error::InvalidFormat("unsupported TCHY version"));
        }
        let bits_per_sample = cursor.read_u8()?;
        let samples_per_chunk = cursor.read_u8()?;
        let endian = match cursor.read_u8()? {
            0 => crate::params::SampleEndian::Little,
            1 => crate::params::SampleEndian::Big,
            _ => return Err(Error::InvalidFormat("invalid endian tag")),
        };
        let chunks_per_split = cursor.read_u16()?;
        let threshold_ppm = cursor.read_u32()?;
        let max_active_raw = cursor.read_u64()?;
        let original_sample_count = cursor.read_u64()? as usize;
        let dictionary_len = cursor.read_u32()? as usize;
        let split_count = cursor.read_u32()? as usize;

        let mut config = TitchyConfig::new(bits_per_sample, samples_per_chunk, chunks_per_split)?;
        config.endian = endian;
        config.new_base_threshold = threshold_ppm as f64 / 1_000_000.0;
        config.max_active_dictionary_bytes = if max_active_raw == u64::MAX {
            None
        } else {
            Some(max_active_raw as usize)
        };
        config.validate()?;

        for _ in 0..split_count {
            let _chunk_start = cursor.read_u64()?;
            let _chunk_count = cursor.read_u32()?;
            let _split_offset = cursor.read_u64()?;
            let _split_len = cursor.read_u32()?;
        }

        let base_byte_len = config.chunk_byte_len();
        let mut dictionary = Vec::with_capacity(dictionary_len);
        for _ in 0..dictionary_len {
            dictionary.push(PackedBits::from_bytes(
                cursor.read_exact(base_byte_len)?.to_vec(),
                config.chunk_bit_len(),
            ));
        }

        let mut splits = Vec::with_capacity(split_count);
        for _ in 0..split_count {
            let l_id = cursor.read_u8()?;
            if l_id > 32 {
                return Err(Error::InvalidFormat("l_id over 32 is not supported"));
            }
            let l_d = cursor.read_u8()?;
            if l_d as usize > config.chunk_bit_len() {
                return Err(Error::InvalidFormat("l_d exceeds chunk bit length"));
            }
            let pair_count = cursor.read_u16()? as usize;
            let pair_byte_len = cursor.read_u32()? as usize;
            let pair_bytes = cursor.read_exact(pair_byte_len)?;
            let pair_bit_len = pair_count * (l_id as usize + l_d as usize);
            let mut reader = BitReader::new(pair_bytes, pair_bit_len);
            let mut pairs = Vec::with_capacity(pair_count);
            for _ in 0..pair_count {
                let base_id = reader.read_value(l_id)? as u32;
                let deviation_bits = reader.read_bits(l_d as usize)?;
                pairs.push(EncodedPair {
                    base_id,
                    deviation_bits,
                });
            }
            splits.push(Split { l_id, l_d, pairs });
        }

        let max_active_dictionary_bases = config
            .max_active_dictionary_bytes
            .map(|bytes| bytes / config.chunk_byte_len().saturating_add(4));

        Ok(Self {
            config,
            original_sample_count,
            dictionary,
            splits,
            max_active_dictionary_bases,
        })
    }

    pub(crate) fn config(&self) -> &TitchyConfig {
        &self.config
    }

    pub(crate) fn split_for_chunk(&self, chunk_index: usize) -> Option<(&Split, usize)> {
        let mut remaining = chunk_index;
        for split in &self.splits {
            if remaining < split.pairs.len() {
                return Some((split, remaining));
            }
            remaining -= split.pairs.len();
        }
        None
    }

    pub(crate) fn reconstruct_chunk(&self, pair: &EncodedPair, l_d: usize) -> Result<Vec<u64>> {
        let base = self
            .dictionary
            .get(pair.base_id as usize)
            .ok_or(Error::DictionaryIdOutOfRange(pair.base_id))?;
        let base_samples = unpack_samples(base, &self.config);
        inverse_transform_chunk(&base_samples, &pair.deviation_bits, l_d, &self.config)
    }
}

fn encode_split(split: &Split) -> Result<Vec<u8>> {
    if split.pairs.len() > u16::MAX as usize {
        return Err(Error::InvalidFormat("split contains too many pairs"));
    }
    let mut out = Vec::new();
    out.push(split.l_id);
    out.push(split.l_d);
    out.extend_from_slice(&(split.pairs.len() as u16).to_le_bytes());
    let mut writer = BitWriter::new();
    for pair in &split.pairs {
        writer.push_value(pair.base_id as u64, split.l_id);
        writer.push_bits(&pair.deviation_bits);
    }
    let pair_bytes = writer.into_bytes();
    out.extend_from_slice(&(pair_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&pair_bytes);
    Ok(out)
}

/// Compresses fixed-width integer samples into an in-memory Titchy container.
pub fn compress_samples(samples: &[u64], config: TitchyConfig) -> Result<CompressedTitchy> {
    config.validate()?;
    let chunks = chunk_samples(samples, &config)?;
    let mut dictionary =
        Dictionary::new(config.chunk_byte_len(), config.max_active_dictionary_bytes);
    let mut persisted_bases = Vec::new();
    let mut splits = Vec::new();
    let mut l_id = ceil_log2(config.chunks_per_split as usize) as u8;
    let mut l_d = (config.chunk_bit_len() / 2) as u8;
    let mut old_base_count = 0usize;
    let mut current_split = Split {
        l_id,
        l_d,
        pairs: Vec::new(),
    };

    for chunk in chunks {
        if current_split.pairs.len() == config.chunks_per_split as usize {
            let global_len = dictionary.len();
            update_parameters(&config, global_len, old_base_count, &mut l_id, &mut l_d);
            old_base_count = global_len;
            splits.push(current_split);
            current_split = Split {
                l_id,
                l_d,
                pairs: Vec::new(),
            };
        }

        let transformed = transform_chunk(&chunk, l_d as usize, &config)?;
        let packed_base = pack_samples(&transformed.base_samples, &config)?;
        let (base_id, is_new) = dictionary.get_or_insert(packed_base.clone());
        if is_new {
            debug_assert_eq!(base_id as usize, persisted_bases.len());
            persisted_bases.push(packed_base);
        }
        current_split.pairs.push(EncodedPair {
            base_id,
            deviation_bits: transformed.deviation_bits,
        });
    }

    if !current_split.pairs.is_empty() {
        splits.push(current_split);
    }

    Ok(CompressedTitchy {
        config,
        original_sample_count: samples.len(),
        dictionary: persisted_bases,
        splits,
        max_active_dictionary_bases: dictionary.max_active_bases(),
    })
}

/// Decompresses every sample and removes final-chunk padding.
pub fn decompress_samples(compressed: &CompressedTitchy) -> Result<Vec<u64>> {
    let mut samples = Vec::new();
    for split in &compressed.splits {
        for pair in &split.pairs {
            samples.extend(compressed.reconstruct_chunk(pair, split.l_d as usize)?);
        }
    }
    samples.truncate(compressed.original_sample_count);
    Ok(samples)
}

/// Compresses byte-aligned samples while preserving their exact bit patterns.
pub fn compress_raw_bytes(raw: &[u8], config: TitchyConfig) -> Result<CompressedTitchy> {
    if !config.bits_per_sample.is_multiple_of(8) {
        return Err(Error::InvalidConfig(
            "raw byte input requires a byte-aligned sample width",
        ));
    }
    let sample_bytes = config.bits_per_sample as usize / 8;
    if !raw.len().is_multiple_of(sample_bytes) {
        return Err(Error::InvalidFormat(
            "raw input length is not a whole number of samples",
        ));
    }
    let mut samples = Vec::with_capacity(raw.len() / sample_bytes);
    for bytes in raw.chunks_exact(sample_bytes) {
        let value = match config.endian {
            crate::params::SampleEndian::Little => {
                bytes.iter().enumerate().fold(0u64, |value, (index, byte)| {
                    value | ((*byte as u64) << (index * 8))
                })
            }
            crate::params::SampleEndian::Big => bytes
                .iter()
                .fold(0u64, |value, byte| (value << 8) | *byte as u64),
        };
        samples.push(value);
    }
    compress_samples(&samples, config)
}

/// Restores the raw byte representation configured in the container.
pub fn decompress_raw_bytes(compressed: &CompressedTitchy) -> Result<Vec<u8>> {
    let config = compressed.config();
    if !config.bits_per_sample.is_multiple_of(8) {
        return Err(Error::InvalidConfig(
            "raw byte output requires a byte-aligned sample width",
        ));
    }
    let sample_bytes = config.bits_per_sample as usize / 8;
    let samples = decompress_samples(compressed)?;
    let mut raw = Vec::with_capacity(samples.len() * sample_bytes);
    for sample in samples {
        match config.endian {
            crate::params::SampleEndian::Little => {
                for index in 0..sample_bytes {
                    raw.push((sample >> (index * 8)) as u8);
                }
            }
            crate::params::SampleEndian::Big => {
                for index in (0..sample_bytes).rev() {
                    raw.push((sample >> (index * 8)) as u8);
                }
            }
        }
    }
    Ok(raw)
}

pub(crate) fn update_parameters(
    config: &TitchyConfig,
    global_base_count: usize,
    old_base_count: usize,
    l_id: &mut u8,
    l_d: &mut u8,
) {
    if capacity_needed(global_base_count, config.chunks_per_split as usize, *l_id) {
        *l_id = l_id.saturating_add(1);
    }

    let new_bases = global_base_count.saturating_sub(old_base_count);
    let f = new_bases as f64 / config.chunks_per_split as f64;
    if new_bases == 0 {
        *l_d = l_d.saturating_sub(1);
    } else if f > config.new_base_threshold {
        let maximum = config.chunk_bit_len().min(u8::MAX as usize) as u8;
        *l_d = (*l_d).saturating_add(1).min(maximum);
    }
}

fn capacity_needed(global_base_count: usize, chunks_per_split: usize, l_id: u8) -> bool {
    if l_id as usize >= usize::BITS as usize {
        return false;
    }
    global_base_count + chunks_per_split >= (1usize << l_id)
}

pub(crate) fn ceil_log2(value: usize) -> usize {
    if value <= 1 {
        0
    } else {
        usize::BITS as usize - (value - 1).leading_zeros() as usize
    }
}

struct ByteCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> ByteCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.pos + len > self.bytes.len() {
            return Err(Error::InvalidFormat("unexpected end of byte stream"));
        }
        let start = self.pos;
        self.pos += len;
        Ok(&self.bytes[start..self.pos])
    }

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16> {
        let bytes = self.read_exact(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32(&mut self) -> Result<u32> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }
}
