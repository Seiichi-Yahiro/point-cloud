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

impl Default for Point {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            color: [0, 0, 0, 255],
        }
    }
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

impl ply_rs::ply::PropertyAccess for Point {
    fn new() -> Self {
        Self::default()
    }

    fn set_property(&mut self, property_name: String, property: ply_rs::ply::Property) {
        use ply_rs::ply::Property;

        match property_name.as_ref() {
            "x" => match property {
                Property::Float(v) => {
                    self.pos.x = v;
                }
                Property::Double(v) => {
                    self.pos.x = v as f32;
                }
                _ => {}
            },
            "y" => match property {
                Property::Float(v) => {
                    self.pos.y = v;
                }
                Property::Double(v) => {
                    self.pos.y = v as f32;
                }
                _ => {}
            },
            "z" => match property {
                Property::Float(v) => {
                    self.pos.z = v;
                }
                Property::Double(v) => {
                    self.pos.z = v as f32;
                }
                _ => {}
            },
            "red" | "r" => match property {
                Property::UChar(v) => {
                    self.color[0] = v;
                }
                Property::Float(v) => {
                    self.color[0] = (v / 255.0) as u8;
                }
                _ => {}
            },
            "green" | "g" => match property {
                Property::UChar(v) => {
                    self.color[1] = v;
                }
                Property::Float(v) => {
                    self.color[1] = (v / 255.0) as u8;
                }
                _ => {}
            },
            "blue" | "b" => match property {
                Property::UChar(v) => {
                    self.color[2] = v;
                }
                Property::Float(v) => {
                    self.color[2] = (v / 255.0) as u8;
                }
                _ => {}
            },
            "alpha" | "a" => match property {
                Property::UChar(v) => {
                    self.color[3] = v;
                }
                Property::Float(v) => {
                    self.color[3] = (v / 255.0) as u8;
                }
                _ => {}
            },
            _ => {}
        }
    }
}
