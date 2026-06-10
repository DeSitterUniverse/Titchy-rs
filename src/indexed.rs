use std::io::{Read, Seek, SeekFrom};

use crate::{
    bitio::BitReader,
    codec::FORMAT_VERSION,
    error::{Error, Result},
    packed_bits::PackedBits,
    params::{SampleEndian, TitchyConfig},
    sample_stream::unpack_samples,
    transform::{deviation_bit_offset, inverse_transform_chunk},
};

const HEADER_LEN: u64 = 38;
const INDEX_ENTRY_LEN: u64 = 24;

#[derive(Debug, Clone)]
struct SplitIndexEntry {
    chunk_start: u64,
    chunk_count: u32,
    split_offset: u64,
    split_len: u32,
}

/// Seek-based reader for the current indexed `TCHY` container.
///
/// Opening reads only metadata and the split index. Access methods seek to the
/// required pair and dictionary bytes.
#[derive(Debug)]
pub struct IndexedTitchyReader<R> {
    reader: R,
    config: TitchyConfig,
    original_sample_count: usize,
    dictionary_len: usize,
    dictionary_offset: u64,
    splits: Vec<SplitIndexEntry>,
}

impl<R: Read + Seek> IndexedTitchyReader<R> {
    /// Opens and validates an indexed container.
    pub fn open(mut reader: R) -> Result<Self> {
        reader.seek(SeekFrom::Start(0))?;
        let mut prefix = [0u8; 5];
        reader.read_exact(&mut prefix)?;
        if &prefix[..4] != b"TCHY" {
            return Err(Error::InvalidFormat("missing TCHY magic"));
        }
        if prefix[4] != FORMAT_VERSION {
            return Err(Error::InvalidFormat("unsupported TCHY version"));
        }
        reader.seek(SeekFrom::Start(0))?;
        let mut header = [0u8; HEADER_LEN as usize];
        reader.read_exact(&mut header)?;
        let bits_per_sample = header[5];
        let samples_per_chunk = header[6];
        let endian = match header[7] {
            0 => SampleEndian::Little,
            1 => SampleEndian::Big,
            _ => return Err(Error::InvalidFormat("invalid endian tag")),
        };
        let chunks_per_split = u16::from_le_bytes([header[8], header[9]]);
        let threshold_ppm = u32::from_le_bytes([header[10], header[11], header[12], header[13]]);
        let max_active_raw = u64::from_le_bytes(header[14..22].try_into().unwrap());
        let original_sample_count = u64::from_le_bytes(header[22..30].try_into().unwrap()) as usize;
        let dictionary_len = u32::from_le_bytes(header[30..34].try_into().unwrap()) as usize;
        let split_count = u32::from_le_bytes(header[34..38].try_into().unwrap()) as usize;

        let mut config = TitchyConfig::new(bits_per_sample, samples_per_chunk, chunks_per_split)?;
        config.endian = endian;
        config.new_base_threshold = threshold_ppm as f64 / 1_000_000.0;
        config.max_active_dictionary_bytes = if max_active_raw == u64::MAX {
            None
        } else {
            Some(max_active_raw as usize)
        };
        config.validate()?;

        let mut splits = Vec::with_capacity(split_count);
        for _ in 0..split_count {
            splits.push(SplitIndexEntry {
                chunk_start: read_u64(&mut reader)?,
                chunk_count: read_u32(&mut reader)?,
                split_offset: read_u64(&mut reader)?,
                split_len: read_u32(&mut reader)?,
            });
        }
        let dictionary_offset = HEADER_LEN + INDEX_ENTRY_LEN * split_count as u64;
        let dictionary_end = dictionary_offset + (dictionary_len * config.chunk_byte_len()) as u64;
        for split in &splits {
            if split.split_offset < dictionary_end || split.split_len < 8 {
                return Err(Error::InvalidFormat("invalid split index entry"));
            }
        }

        Ok(Self {
            reader,
            config,
            original_sample_count,
            dictionary_len,
            dictionary_offset,
            splits,
        })
    }

    /// Retrieves one sample without decoding unrelated chunks.
    pub fn get_by_sample_index(&mut self, index: usize) -> Result<u64> {
        if index >= self.original_sample_count {
            return Err(Error::SampleIndexOutOfRange {
                index,
                len: self.original_sample_count,
            });
        }
        let chunk_index = index / self.config.samples_per_chunk as usize;
        let sample_in_chunk = index % self.config.samples_per_chunk as usize;
        let chunk = self.read_chunk(chunk_index)?;
        Ok(chunk[sample_in_chunk])
    }

    /// Retrieves one MSB-first bit from the original sample stream.
    pub fn get_by_bit_index(&mut self, bit_index: usize) -> Result<u8> {
        let bits_per_sample = self.config.bits_per_sample as usize;
        let bit_len = self
            .original_sample_count
            .checked_mul(bits_per_sample)
            .ok_or(Error::InvalidFormat("uncompressed bit length overflow"))?;
        if bit_index >= bit_len {
            return Err(Error::BitIndexOutOfRange {
                index: bit_index,
                len: bit_len,
            });
        }

        let sample_index = bit_index / bits_per_sample;
        let bit_in_sample = bit_index % bits_per_sample;
        let chunk_index = sample_index / self.config.samples_per_chunk as usize;
        let sample_in_chunk = sample_index % self.config.samples_per_chunk as usize;
        let split = self.split_for_chunk(chunk_index)?;
        let local_index = chunk_index - split.chunk_start as usize;
        let (l_id, l_d, payload_len) = self.read_split_header(&split, local_index)?;
        let pair_bit_len = l_id as usize + l_d as usize;
        let pair_start = local_index * pair_bit_len;

        if let Some(deviation_offset) =
            deviation_bit_offset(&self.config, l_d as usize, sample_in_chunk, bit_in_sample)
        {
            return Ok(self.read_payload_value(
                &split,
                payload_len,
                pair_start + l_id as usize + deviation_offset,
                1,
            )? as u8);
        }

        let base_id =
            self.read_payload_value(&split, payload_len, pair_start, l_id as usize)? as usize;
        if base_id >= self.dictionary_len {
            return Err(Error::InvalidFormat("base ID points outside dictionary"));
        }
        let base_bit = sample_in_chunk * bits_per_sample + bit_in_sample;
        let byte_offset = base_bit / 8;
        self.reader.seek(SeekFrom::Start(
            self.dictionary_offset + (base_id * self.config.chunk_byte_len() + byte_offset) as u64,
        ))?;
        let mut byte = [0u8; 1];
        self.reader.read_exact(&mut byte)?;
        Ok((byte[0] >> (7 - base_bit % 8)) & 1)
    }

    /// Retrieves a contiguous sample range.
    pub fn get_range_by_sample_index(&mut self, start: usize, len: usize) -> Result<Vec<u64>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let end = start.checked_add(len).ok_or(Error::SampleIndexOutOfRange {
            index: usize::MAX,
            len: self.original_sample_count,
        })?;
        if end > self.original_sample_count {
            return Err(Error::SampleIndexOutOfRange {
                index: end - 1,
                len: self.original_sample_count,
            });
        }

        let c = self.config.samples_per_chunk as usize;
        let first_chunk = start / c;
        let last_chunk = (end - 1) / c;
        let mut decoded = Vec::with_capacity((last_chunk - first_chunk + 1) * c);
        for chunk_index in first_chunk..=last_chunk {
            decoded.extend(self.read_chunk(chunk_index)?);
        }
        let offset = start - first_chunk * c;
        Ok(decoded[offset..offset + len].to_vec())
    }

    /// Returns the original unpadded sample count.
    pub fn original_sample_count(&self) -> usize {
        self.original_sample_count
    }

    /// Consumes the indexed reader and returns its underlying reader.
    pub fn into_inner(self) -> R {
        self.reader
    }

    fn read_chunk(&mut self, chunk_index: usize) -> Result<Vec<u64>> {
        let split = self.split_for_chunk(chunk_index)?;
        let local_index = chunk_index - split.chunk_start as usize;
        let (l_id, l_d, payload_len) = self.read_split_header(&split, local_index)?;

        let pair_bit_len = l_id as usize + l_d as usize;
        let pair_start = local_index * pair_bit_len;
        let byte_start = pair_start / 8;
        let leading_bits = pair_start % 8;
        let bytes_needed = (leading_bits + pair_bit_len).div_ceil(8);
        if byte_start + bytes_needed > payload_len {
            return Err(Error::InvalidFormat("pair points outside split payload"));
        }
        self.reader
            .seek(SeekFrom::Start(split.split_offset + 8 + byte_start as u64))?;
        let mut pair_bytes = vec![0u8; bytes_needed];
        self.reader.read_exact(&mut pair_bytes)?;
        let mut pair_reader = BitReader::new(&pair_bytes, bytes_needed * 8);
        pair_reader.read_bits(leading_bits)?;
        let base_id = pair_reader.read_value(l_id)? as usize;
        let deviation = pair_reader.read_bits(l_d as usize)?;
        if base_id >= self.dictionary_len {
            return Err(Error::InvalidFormat("base ID points outside dictionary"));
        }

        self.reader.seek(SeekFrom::Start(
            self.dictionary_offset + (base_id * self.config.chunk_byte_len()) as u64,
        ))?;
        let mut base_bytes = vec![0u8; self.config.chunk_byte_len()];
        self.reader.read_exact(&mut base_bytes)?;
        let base = PackedBits::from_bytes(base_bytes, self.config.chunk_bit_len());
        let base_samples = unpack_samples(&base, &self.config);
        inverse_transform_chunk(&base_samples, &deviation, l_d as usize, &self.config)
    }

    fn split_for_chunk(&self, chunk_index: usize) -> Result<SplitIndexEntry> {
        self.splits
            .iter()
            .find(|split| {
                let start = split.chunk_start as usize;
                chunk_index >= start && chunk_index < start + split.chunk_count as usize
            })
            .cloned()
            .ok_or(Error::InvalidFormat("chunk is not covered by split index"))
    }

    fn read_split_header(
        &mut self,
        split: &SplitIndexEntry,
        local_index: usize,
    ) -> Result<(u8, u8, usize)> {
        self.reader.seek(SeekFrom::Start(split.split_offset))?;
        let mut split_header = [0u8; 8];
        self.reader.read_exact(&mut split_header)?;
        let l_id = split_header[0];
        let l_d = split_header[1];
        let pair_count = u16::from_le_bytes([split_header[2], split_header[3]]) as usize;
        let payload_len = u32::from_le_bytes(split_header[4..8].try_into().unwrap()) as usize;
        if pair_count != split.chunk_count as usize
            || local_index >= pair_count
            || l_id > 32
            || l_d as usize > self.config.chunk_bit_len()
            || payload_len + 8 > split.split_len as usize
        {
            return Err(Error::InvalidFormat("split record does not match index"));
        }
        Ok((l_id, l_d, payload_len))
    }

    fn read_payload_value(
        &mut self,
        split: &SplitIndexEntry,
        payload_len: usize,
        bit_start: usize,
        bit_len: usize,
    ) -> Result<u64> {
        if bit_len > 64
            || bit_start
                .checked_add(bit_len)
                .is_none_or(|end| end > payload_len * 8)
        {
            return Err(Error::InvalidFormat("bit points outside split payload"));
        }
        if bit_len == 0 {
            return Ok(0);
        }

        let byte_start = bit_start / 8;
        let leading_bits = bit_start % 8;
        let bytes_needed = (leading_bits + bit_len).div_ceil(8);
        self.reader
            .seek(SeekFrom::Start(split.split_offset + 8 + byte_start as u64))?;
        let mut bytes = vec![0u8; bytes_needed];
        self.reader.read_exact(&mut bytes)?;
        let mut reader = BitReader::new(&bytes, bytes_needed * 8);
        reader.read_bits(leading_bits)?;
        reader.read_value(bit_len as u8)
    }
}

fn read_u32(reader: &mut impl Read) -> Result<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> Result<u64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}
