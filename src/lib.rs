//! Lossless Titchy compression for fixed-width time-series samples.
//!
//! The crate provides batch compression, online packet streaming, bounded
//! active dictionaries, binary serialization, and indexed bit/sample/range
//! access. Start with [`compress_samples`] and [`decompress_samples`], or use
//! [`IndexedTitchyReader`] for seek-based access to a serialized container.

mod bitio;
mod codec;
mod dictionary;
mod error;
mod indexed;
mod metrics;
mod packed_bits;
mod params;
mod random_access;
mod sample_stream;
mod streaming;
mod synthetic;
mod transform;

pub use codec::{
    compress_raw_bytes, compress_samples, decompress_raw_bytes, decompress_samples,
    CompressedTitchy, SplitMetadata,
};
pub use error::{Error, Result};
pub use indexed::IndexedTitchyReader;
pub use metrics::{
    bit_access_cost, expected_block_count, sample_access_cost, titchy_access_bounds,
    universal_access_cost, AccessCostBounds,
};
pub use params::{SampleEndian, TitchyConfig};
pub use random_access::{get_by_bit_index, get_by_sample_index, get_range_by_sample_index};
pub use streaming::{StreamCollector, StreamDecoder, StreamEncoder, StreamPacket};
pub use synthetic::synthetic_sensor_samples;
pub use transform::{inverse_transform_chunk, transform_chunk, TransformedChunk};
