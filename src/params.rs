use crate::error::{Error, Result};

/// Byte order used when converting byte-aligned raw samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleEndian {
    /// Least-significant sample byte first.
    Little,
    /// Most-significant sample byte first.
    Big,
}

/// Compression parameters shared by batch and streaming encoders.
#[derive(Debug, Clone, PartialEq)]
pub struct TitchyConfig {
    /// Fixed width of one sample, from 1 through 64 bits.
    pub bits_per_sample: u8,
    /// Number of samples transformed together as one chunk.
    pub samples_per_chunk: u8,
    /// Number of chunks encoded with one pair of adaptive parameters.
    pub chunks_per_split: u16,
    /// Fraction of new bases above which the deviation width grows.
    pub new_base_threshold: f64,
    /// Byte order for raw byte conversion.
    pub endian: SampleEndian,
    /// Logical active-dictionary byte limit, or `None` for unlimited.
    pub max_active_dictionary_bytes: Option<usize>,
}

impl TitchyConfig {
    /// Creates a validated configuration with paper-default threshold and
    /// unlimited active dictionary memory.
    pub fn new(bits_per_sample: u8, samples_per_chunk: u8, chunks_per_split: u16) -> Result<Self> {
        let config = Self {
            bits_per_sample,
            samples_per_chunk,
            chunks_per_split,
            new_base_threshold: 0.25,
            endian: SampleEndian::Little,
            max_active_dictionary_bytes: None,
        };
        config.validate()?;
        Ok(config)
    }

    /// Sets the raw-sample byte order.
    pub fn with_endian(mut self, endian: SampleEndian) -> Self {
        self.endian = endian;
        self
    }

    /// Sets the logical active-dictionary memory limit.
    pub fn with_max_active_dictionary_bytes(mut self, bytes: Option<usize>) -> Self {
        self.max_active_dictionary_bytes = bytes;
        self
    }

    /// Returns the number of meaningful bits in one chunk.
    pub fn chunk_bit_len(&self) -> usize {
        self.bits_per_sample as usize * self.samples_per_chunk as usize
    }

    /// Returns the bytes required to store one packed chunk base.
    pub fn chunk_byte_len(&self) -> usize {
        self.chunk_bit_len().div_ceil(8)
    }

    /// Validates all configuration invariants.
    pub fn validate(&self) -> Result<()> {
        if self.bits_per_sample == 0 || self.bits_per_sample > 64 {
            return Err(Error::InvalidConfig("bits_per_sample must be in 1..=64"));
        }
        if self.samples_per_chunk == 0 {
            return Err(Error::InvalidConfig("samples_per_chunk must be non-zero"));
        }
        if self.chunks_per_split == 0 {
            return Err(Error::InvalidConfig("chunks_per_split must be non-zero"));
        }
        if self.chunk_bit_len() / 2 > u8::MAX as usize {
            return Err(Error::InvalidConfig(
                "initial deviation length must fit in the one-byte l_d field",
            ));
        }
        if !(0.0..=1.0).contains(&self.new_base_threshold) {
            return Err(Error::InvalidConfig(
                "new_base_threshold must be between 0 and 1",
            ));
        }
        Ok(())
    }
}
