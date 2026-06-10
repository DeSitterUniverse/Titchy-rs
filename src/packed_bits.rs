#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackedBits {
    bytes: Vec<u8>,
    bit_len: usize,
}

impl PackedBits {
    pub fn from_bits(bits: &[u8]) -> Self {
        let mut bytes = vec![0; bits.len().div_ceil(8)];
        for (i, bit) in bits.iter().enumerate() {
            if bit & 1 == 1 {
                bytes[i / 8] |= 1 << (7 - (i % 8));
            }
        }
        Self {
            bytes,
            bit_len: bits.len(),
        }
    }

    pub fn from_bytes(bytes: Vec<u8>, bit_len: usize) -> Self {
        let mut bytes = bytes;
        if !bit_len.is_multiple_of(8) {
            let used = bit_len % 8;
            if let Some(last) = bytes.last_mut() {
                *last &= 0xff << (8 - used);
            }
        }
        Self { bytes, bit_len }
    }

    pub fn bit_len(&self) -> usize {
        self.bit_len
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn get_bit(&self, index: usize) -> u8 {
        if index >= self.bit_len {
            return 0;
        }
        (self.bytes[index / 8] >> (7 - (index % 8))) & 1
    }
}
