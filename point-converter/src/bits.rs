use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

#[derive(Debug)]
pub struct Bits {
    bits: Vec<u32>,
}

impl Bits {
    pub fn new(capacity: usize) -> Self {
        Self {
            bits: vec![0u32; (capacity as f32 / 32.0).ceil() as usize],
        }
    }

    /// Set the bit at the given index to 1.
    /// Returns true if the bit was unset before.
    pub fn set_bit(&mut self, index: usize) -> bool {
        self.validate_index(index);
        let bits = &mut self.bits[index / 32];
        let bit = 1u32 << (index % 32) as u32;

        let will_write = *bits & bit == 0;

        *bits |= bit;

        will_write
    }

    pub fn is_bit_set(&self, index: usize) -> bool {
        self.validate_index(index);
        self.bits[index / 32] & (1u32 << (index % 32) as u32) != 0
    }

    pub fn unset_bit(&mut self, index: usize) {
        self.validate_index(index);
        if self.is_bit_set(index) {
            self.bits[index / 32] ^= 1u32 << (index % 32) as u32;
        }
    }

    fn validate_index(&self, index: usize) {
        let number_of_bits = self.bits.len() * 32;

        if index > number_of_bits {
            panic!(
                "Tried to access bit at index {} but only {} bits exist",
                index, number_of_bits
            )
        }
    }

    pub fn write_to(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        for bits in &self.bits {
            writer.write_u32::<BigEndian>(*bits)?;
        }

        Ok(())
    }

    pub fn read_from(reader: &mut dyn Read, capacity: usize) -> Result<Self, std::io::Error> {
        let mut grid = Self::new(capacity);
        reader.read_u32_into::<BigEndian>(&mut grid.bits)?;
        Ok(grid)
    }
}
