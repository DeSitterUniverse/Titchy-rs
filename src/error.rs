use std::fmt::{Display, Formatter};

/// Crate result type using [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by Titchy configuration, codecs, formats, and access APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A configuration invariant was violated.
    InvalidConfig(&'static str),
    /// A sample does not fit the configured bit width.
    InvalidSample { sample: u64, bits_per_sample: u8 },
    /// A transform received the wrong number of samples.
    InvalidChunkLen { expected: usize, actual: usize },
    /// A transform received the wrong number of deviation bits.
    InvalidDeviationLen { expected: usize, actual: usize },
    /// A pair references an unavailable dictionary ID.
    DictionaryIdOutOfRange(u32),
    /// Serialized container bytes are malformed or unsupported.
    InvalidFormat(&'static str),
    /// Streaming packet bytes or ordering are invalid.
    InvalidStream(&'static str),
    /// An underlying I/O operation failed.
    Io(String),
    /// A sample index falls outside the original stream.
    SampleIndexOutOfRange { index: usize, len: usize },
    /// A bit index falls outside the original stream.
    BitIndexOutOfRange { index: usize, len: usize },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidConfig(message) => write!(f, "invalid config: {message}"),
            Error::InvalidSample {
                sample,
                bits_per_sample,
            } => write!(
                f,
                "sample value {sample} does not fit in {bits_per_sample} bits"
            ),
            Error::InvalidChunkLen { expected, actual } => {
                write!(f, "invalid chunk length: expected {expected}, got {actual}")
            }
            Error::InvalidDeviationLen { expected, actual } => write!(
                f,
                "invalid deviation length: expected {expected}, got {actual}"
            ),
            Error::DictionaryIdOutOfRange(id) => write!(f, "dictionary ID {id} is out of range"),
            Error::InvalidFormat(message) => write!(f, "invalid format: {message}"),
            Error::InvalidStream(message) => write!(f, "invalid stream: {message}"),
            Error::Io(message) => write!(f, "I/O error: {message}"),
            Error::SampleIndexOutOfRange { index, len } => {
                write!(f, "sample index {index} is out of range for length {len}")
            }
            Error::BitIndexOutOfRange { index, len } => {
                write!(f, "bit index {index} is out of range for length {len}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}
