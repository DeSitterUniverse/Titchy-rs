use crate::{
    error::{Error, Result},
    packed_bits::PackedBits,
};

#[derive(Debug, Default)]
pub struct BitWriter {
    bits: Vec<u8>,
}

impl BitWriter {
    pub fn new() -> Self {
        Self { bits: Vec::new() }
    }

    pub fn push_bit(&mut self, bit: u8) {
        self.bits.push(bit & 1);
    }

    pub fn push_bits(&mut self, bits: &[u8]) {
        for &bit in bits {
            self.push_bit(bit);
        }
    }

    pub fn push_value(&mut self, value: u64, bit_len: u8) {
        for shift in (0..bit_len).rev() {
            self.push_bit(((value >> shift) & 1) as u8);
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        PackedBits::from_bits(&self.bits).bytes().to_vec()
    }
}

#[derive(Debug)]
pub struct BitReader<'a> {
    bytes: &'a [u8],
    bit_len: usize,
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(bytes: &'a [u8], bit_len: usize) -> Self {
        Self {
            bytes,
            bit_len,
            pos: 0,
        }
    }

    pub fn read_bit(&mut self) -> Result<u8> {
        if self.pos >= self.bit_len {
            return Err(Error::InvalidFormat("unexpected end of bit stream"));
        }
        let bit = (self.bytes[self.pos / 8] >> (7 - (self.pos % 8))) & 1;
        self.pos += 1;
        Ok(bit)
    }

    pub fn read_bits(&mut self, bit_len: usize) -> Result<Vec<u8>> {
        let mut bits = Vec::with_capacity(bit_len);
        for _ in 0..bit_len {
            bits.push(self.read_bit()?);
        }
        Ok(bits)
    }

    pub fn read_value(&mut self, bit_len: u8) -> Result<u64> {
        if bit_len > 64 {
            return Err(Error::InvalidFormat(
                "cannot read values wider than 64 bits",
            ));
        }
        let mut value = 0u64;
        for _ in 0..bit_len {
            value = (value << 1) | self.read_bit()? as u64;
        }
        Ok(value)
    }
}
