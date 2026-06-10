use std::collections::VecDeque;

use crate::{
    bitio::{BitReader, BitWriter},
    codec::{ceil_log2, update_parameters, CompressedTitchy, EncodedPair, Split},
    dictionary::Dictionary,
    error::{Error, Result},
    packed_bits::PackedBits,
    params::TitchyConfig,
    sample_stream::{pack_samples, unpack_samples, validate_sample},
    transform::{inverse_transform_chunk, transform_chunk},
};

/// One paper-compatible online packet plus transport-provided chunk count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPacket {
    bytes: Vec<u8>,
    chunk_count: usize,
}

impl StreamPacket {
    /// Wraps packet bytes received with an external chunk count.
    pub fn from_bytes(bytes: Vec<u8>, chunk_count: usize) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::InvalidStream("empty packet"));
        }
        if chunk_count == 0 {
            return Err(Error::InvalidStream("packet chunk count must be non-zero"));
        }
        Ok(Self { bytes, chunk_count })
    }

    /// Returns the encoded packet payload.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the number of chunk pairs in the packet.
    pub fn chunk_count(&self) -> usize {
        self.chunk_count
    }

    /// Reports whether the packet carries updated adaptive parameters.
    pub fn parameters_present(&self) -> bool {
        self.bytes.first().is_some_and(|byte| byte & 0x80 != 0)
    }
}

#[derive(Debug, Default)]
struct PendingPacket {
    parameters_present: bool,
    new_bases: Vec<PackedBits>,
    pairs: Vec<EncodedPair>,
}

/// Incremental online Titchy encoder.
pub struct StreamEncoder {
    config: TitchyConfig,
    chunks_per_packet: usize,
    dictionary: Dictionary,
    sample_buffer: Vec<u64>,
    current_split_chunks: usize,
    old_base_count: usize,
    l_id: u8,
    l_d: u8,
    pending: PendingPacket,
    ready: VecDeque<StreamPacket>,
}

impl StreamEncoder {
    /// Creates an encoder that flushes at the requested packet chunk target.
    pub fn new(config: TitchyConfig, chunks_per_packet: usize) -> Result<Self> {
        config.validate()?;
        if chunks_per_packet == 0 {
            return Err(Error::InvalidConfig("chunks_per_packet must be non-zero"));
        }
        let l_id = ceil_log2(config.chunks_per_split as usize) as u8;
        let l_d = (config.chunk_bit_len() / 2) as u8;
        let dictionary =
            Dictionary::new(config.chunk_byte_len(), config.max_active_dictionary_bytes);
        Ok(Self {
            config,
            chunks_per_packet,
            dictionary,
            sample_buffer: Vec::new(),
            current_split_chunks: 0,
            old_base_count: 0,
            l_id,
            l_d,
            pending: PendingPacket {
                parameters_present: true,
                ..PendingPacket::default()
            },
            ready: VecDeque::new(),
        })
    }

    /// Pushes one sample and returns a packet when a flush boundary is reached.
    pub fn push_sample(&mut self, sample: u64) -> Result<Option<StreamPacket>> {
        validate_sample(sample, self.config.bits_per_sample)?;
        self.sample_buffer.push(sample);
        if self.sample_buffer.len() == self.config.samples_per_chunk as usize {
            let chunk = std::mem::take(&mut self.sample_buffer);
            self.encode_chunk(&chunk)?;
        }
        Ok(self.ready.pop_front())
    }

    /// Pads any partial final chunk and returns all remaining packets.
    pub fn finish(mut self) -> Result<Vec<StreamPacket>> {
        if !self.sample_buffer.is_empty() {
            let mut chunk = std::mem::take(&mut self.sample_buffer);
            chunk.resize(self.config.samples_per_chunk as usize, 0);
            self.encode_chunk(&chunk)?;
        }
        self.flush_pending()?;
        Ok(self.ready.into_iter().collect())
    }

    fn encode_chunk(&mut self, chunk: &[u64]) -> Result<()> {
        if self.current_split_chunks == self.config.chunks_per_split as usize {
            self.flush_pending()?;
            let global_len = self.dictionary.len();
            update_parameters(
                &self.config,
                global_len,
                self.old_base_count,
                &mut self.l_id,
                &mut self.l_d,
            );
            self.old_base_count = global_len;
            self.current_split_chunks = 0;
            self.pending.parameters_present = true;
        }

        let transformed = transform_chunk(chunk, self.l_d as usize, &self.config)?;
        let packed_base = pack_samples(&transformed.base_samples, &self.config)?;
        let (base_id, is_new) = self.dictionary.get_or_insert(packed_base.clone());
        if is_new {
            self.pending.new_bases.push(packed_base);
        }
        self.pending.pairs.push(EncodedPair {
            base_id,
            deviation_bits: transformed.deviation_bits,
        });
        self.current_split_chunks += 1;

        if self.pending.pairs.len() == self.chunks_per_packet || self.pending.new_bases.len() == 127
        {
            self.flush_pending()?;
        }
        Ok(())
    }

    fn flush_pending(&mut self) -> Result<()> {
        if self.pending.pairs.is_empty() {
            return Ok(());
        }
        let pending = std::mem::take(&mut self.pending);
        let new_base_count = pending.new_bases.len();
        if new_base_count > 127 {
            return Err(Error::InvalidStream(
                "packet cannot contain more than 127 new bases",
            ));
        }

        let mut bytes = Vec::new();
        let header = if pending.parameters_present { 0x80 } else { 0 } | new_base_count as u8;
        bytes.push(header);
        if pending.parameters_present {
            bytes.push(self.l_id);
            bytes.push(self.l_d);
        }
        let mut writer = BitWriter::new();
        for base in &pending.new_bases {
            for index in 0..base.bit_len() {
                writer.push_bit(base.get_bit(index));
            }
        }
        for pair in &pending.pairs {
            writer.push_value(pair.base_id as u64, self.l_id);
            writer.push_bits(&pair.deviation_bits);
        }
        bytes.extend_from_slice(&writer.into_bytes());
        self.ready.push_back(StreamPacket {
            bytes,
            chunk_count: pending.pairs.len(),
        });
        Ok(())
    }
}

/// Incremental decoder for paper-compatible packets.
pub struct StreamDecoder {
    config: TitchyConfig,
    dictionary: Vec<PackedBits>,
    parameters: Option<(u8, u8)>,
}

/// Collects packets into a random-access container without sample decoding.
pub struct StreamCollector {
    config: TitchyConfig,
    dictionary: Vec<PackedBits>,
    splits: Vec<Split>,
    current_split: Option<Split>,
}

impl StreamCollector {
    /// Creates an empty packet collector.
    pub fn new(config: TitchyConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            dictionary: Vec::new(),
            splits: Vec::new(),
            current_split: None,
        })
    }

    /// Validates and appends one packet.
    pub fn ingest_packet(&mut self, packet: &StreamPacket) -> Result<()> {
        let header = *packet
            .bytes
            .first()
            .ok_or(Error::InvalidStream("empty packet"))?;
        let parameters_present = header & 0x80 != 0;
        let new_base_count = (header & 0x7f) as usize;
        let mut byte_offset = 1;
        if parameters_present {
            if packet.bytes.len() < 3 {
                return Err(Error::InvalidStream("truncated parameter fields"));
            }
            if let Some(existing) = self.current_split.as_ref() {
                if !existing.pairs.is_empty()
                    && existing.pairs.len() != self.config.chunks_per_split as usize
                {
                    return Err(Error::InvalidStream(
                        "parameters changed before the split was complete",
                    ));
                }
            }
            if self
                .current_split
                .as_ref()
                .is_some_and(|split| !split.pairs.is_empty())
            {
                if let Some(split) = self.current_split.take() {
                    self.splits.push(split);
                }
            }
            let l_id = packet.bytes[1];
            let l_d = packet.bytes[2];
            if l_id > 32 || l_d as usize > self.config.chunk_bit_len() {
                return Err(Error::InvalidStream("invalid stream parameters"));
            }
            self.current_split = Some(Split {
                l_id,
                l_d,
                pairs: Vec::new(),
            });
            byte_offset = 3;
        }
        let current = self
            .current_split
            .as_mut()
            .ok_or(Error::InvalidStream("parameters required before pairs"))?;
        if current.pairs.len() + packet.chunk_count > self.config.chunks_per_split as usize {
            return Err(Error::InvalidStream(
                "packet would exceed configured split size",
            ));
        }

        let payload = &packet.bytes[byte_offset..];
        let payload_bit_len = new_base_count * self.config.chunk_bit_len()
            + packet.chunk_count * (current.l_id as usize + current.l_d as usize);
        if payload.len() * 8 < payload_bit_len {
            return Err(Error::InvalidStream("truncated packet payload"));
        }
        let mut reader = BitReader::new(payload, payload_bit_len);
        for _ in 0..new_base_count {
            self.dictionary.push(PackedBits::from_bits(
                &reader.read_bits(self.config.chunk_bit_len())?,
            ));
        }
        for _ in 0..packet.chunk_count {
            let base_id = reader.read_value(current.l_id)? as u32;
            if base_id as usize >= self.dictionary.len() {
                return Err(Error::InvalidStream("base ID is not available"));
            }
            current.pairs.push(EncodedPair {
                base_id,
                deviation_bits: reader.read_bits(current.l_d as usize)?,
            });
        }
        Ok(())
    }

    /// Finishes collection with the original unpadded sample count.
    pub fn finish(mut self, original_sample_count: usize) -> Result<CompressedTitchy> {
        if let Some(split) = self.current_split.take() {
            if !split.pairs.is_empty() {
                self.splits.push(split);
            }
        }
        let encoded_samples = self
            .splits
            .iter()
            .map(|split| split.pairs.len())
            .sum::<usize>()
            * self.config.samples_per_chunk as usize;
        if original_sample_count > encoded_samples
            || (encoded_samples > 0
                && original_sample_count
                    <= encoded_samples.saturating_sub(self.config.samples_per_chunk as usize))
        {
            return Err(Error::InvalidStream(
                "original sample count is inconsistent with encoded chunks",
            ));
        }
        let max_active_dictionary_bases = self
            .config
            .max_active_dictionary_bytes
            .map(|bytes| bytes / self.config.chunk_byte_len().saturating_add(4));
        Ok(CompressedTitchy {
            config: self.config,
            original_sample_count,
            dictionary: self.dictionary,
            splits: self.splits,
            max_active_dictionary_bases,
        })
    }
}

impl StreamDecoder {
    /// Creates an empty streaming decoder.
    pub fn new(config: TitchyConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            dictionary: Vec::new(),
            parameters: None,
        })
    }

    /// Decodes one packet and returns its complete padded chunks.
    pub fn decode_packet(&mut self, packet: &StreamPacket) -> Result<Vec<u64>> {
        let header = *packet
            .bytes
            .first()
            .ok_or(Error::InvalidStream("empty packet"))?;
        let parameters_present = header & 0x80 != 0;
        let new_base_count = (header & 0x7f) as usize;
        let mut byte_offset = 1;
        if parameters_present {
            if packet.bytes.len() < 3 {
                return Err(Error::InvalidStream("truncated parameter fields"));
            }
            let l_id = packet.bytes[1];
            let l_d = packet.bytes[2];
            if l_id > 32 || l_d as usize > self.config.chunk_bit_len() {
                return Err(Error::InvalidStream("invalid stream parameters"));
            }
            self.parameters = Some((l_id, l_d));
            byte_offset = 3;
        }
        let (l_id, l_d) = self
            .parameters
            .ok_or(Error::InvalidStream("parameters required before pairs"))?;

        let payload = &packet.bytes[byte_offset..];
        let payload_bit_len = new_base_count * self.config.chunk_bit_len()
            + packet.chunk_count * (l_id as usize + l_d as usize);
        if payload.len() * 8 < payload_bit_len {
            return Err(Error::InvalidStream("truncated packet payload"));
        }
        let mut reader = BitReader::new(payload, payload_bit_len);
        for _ in 0..new_base_count {
            let bits = reader.read_bits(self.config.chunk_bit_len())?;
            self.dictionary.push(PackedBits::from_bits(&bits));
        }

        let mut samples =
            Vec::with_capacity(packet.chunk_count * self.config.samples_per_chunk as usize);
        for _ in 0..packet.chunk_count {
            let base_id = reader.read_value(l_id)? as usize;
            let deviation = reader.read_bits(l_d as usize)?;
            let base = self
                .dictionary
                .get(base_id)
                .ok_or(Error::InvalidStream("base ID is not available"))?;
            let base_samples = unpack_samples(base, &self.config);
            samples.extend(inverse_transform_chunk(
                &base_samples,
                &deviation,
                l_d as usize,
                &self.config,
            )?);
        }
        Ok(samples)
    }
}
