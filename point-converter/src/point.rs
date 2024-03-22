use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use glam::Vec3;

#[derive(Debug, Copy, Clone)]
pub struct Point {
    /// Position of the point in 3D Space.
    pub pos: Vec3,
    /// RGBA color value 0..=255.
    pub color: [u8; 4],
}

impl Point {
    pub fn write_to(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        writer.write_f32::<BigEndian>(self.pos.x)?;
        writer.write_f32::<BigEndian>(self.pos.y)?;
        writer.write_f32::<BigEndian>(self.pos.z)?;

        writer.write_u8(self.color[0])?;
        writer.write_u8(self.color[1])?;
        writer.write_u8(self.color[2])?;
        writer.write_u8(self.color[3])?;

        Ok(())
    }

    pub fn read_from(reader: &mut dyn Read) -> Result<Self, std::io::Error> {
        let x = reader.read_f32::<BigEndian>()?;
        let y = reader.read_f32::<BigEndian>()?;
        let z = reader.read_f32::<BigEndian>()?;

        let r = reader.read_u8()?;
        let g = reader.read_u8()?;
        let b = reader.read_u8()?;
        let a = reader.read_u8()?;

        Ok(Self {
            pos: Vec3::new(x, y, z),
            color: [r, g, b, a],
        })
    }
}
