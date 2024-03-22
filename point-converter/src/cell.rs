use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use glam::{UVec3, Vec3};

use crate::bits::Bits;
use crate::metadata::Metadata;
use crate::point::Point;

#[derive(Debug)]
pub struct Cell {
    header: Header,
    points: Vec<Point>,
    overflow_points: Vec<Point>,
}

impl Cell {
    pub fn new(capacity: usize) -> Self {
        Self {
            header: Header::new(capacity),
            points: Vec::new(),
            overflow_points: Vec::new(),
        }
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn overflow_points(&self) -> &[Point] {
        &self.overflow_points
    }

    pub fn add_point(
        &mut self,
        point: Point,
        cell_size: f32,
        cell_pos: Vec3,
        metadata: &Metadata,
    ) -> Result<(), CellAddPointError> {
        let sub_cell_size = cell_size / metadata.sub_grid_dimension as f32;
        let offset = point.pos - cell_pos + cell_size / 2.0;

        let sub_cell_id = (offset / sub_cell_size)
            .as_uvec3()
            .min(UVec3::splat(metadata.sub_grid_dimension - 1)); // TODO why is min needed? precision problem? or bug?
        let linear_sub_cell_id = sub_cell_id.x
            + sub_cell_id.y * metadata.sub_grid_dimension
            + sub_cell_id.z * metadata.sub_grid_dimension.pow(2);

        if self.header.grid.set_bit(linear_sub_cell_id as usize) {
            self.header.number_of_points += 1;
            self.points.push(point);

            Ok(())
        } else if let Some(overflow) = &mut self.header.overflow {
            if *overflow < metadata.cell_point_overflow_limit {
                *overflow += 1;
                self.overflow_points.push(point);

                Ok(())
            } else {
                Err(CellAddPointError::PointLimitReached)
            }
        } else {
            Err(CellAddPointError::GridPositionOccupied)
        }
    }

    pub fn extract_and_close_overflow(&mut self) -> Vec<Point> {
        self.header.overflow = None;
        std::mem::take(&mut self.overflow_points)
    }

    pub fn write_to(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        self.header.write_to(writer)?;

        for point in &self.points {
            point.write_to(writer)?;
        }

        if self.header.overflow.is_some() {
            for point in &self.overflow_points {
                point.write_to(writer)?;
            }
        }

        Ok(())
    }

    pub fn read_from(reader: &mut dyn Read, metadata: &Metadata) -> Result<Self, std::io::Error> {
        let header = Header::read_from(reader, metadata)?;

        let mut points = Vec::with_capacity(header.number_of_points as usize);

        for _ in 0..header.number_of_points {
            let point = Point::read_from(reader)?;
            points.push(point);
        }

        let overflow_points = if let Some(overflow) = header.overflow {
            let mut points = Vec::with_capacity(overflow as usize);

            for _ in 0..overflow {
                let point = Point::read_from(reader)?;
                points.push(point);
            }

            points
        } else {
            Vec::new()
        };

        Ok(Self {
            header,
            points,
            overflow_points,
        })
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CellAddPointError {
    PointLimitReached,
    GridPositionOccupied,
}

#[derive(Debug)]
pub struct Header {
    /// Number of points in this cell.
    number_of_points: u32,

    /// When some then number of points overflowing this cell.
    /// When none then there is no overflow.
    overflow: Option<u32>,

    /// Grid-bit-mask that signifies if a point exists at a grid location.
    grid: Bits,
}

impl Header {
    pub fn new(capacity: usize) -> Self {
        Self {
            number_of_points: 0,
            overflow: Some(0),
            grid: Bits::new(capacity),
        }
    }

    pub fn write_to(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        writer.write_u32::<BigEndian>(self.number_of_points)?;

        if let Some(overflow) = self.overflow {
            writer.write_u8(1)?;
            writer.write_u32::<BigEndian>(overflow)?;
        } else {
            writer.write_u8(0)?;
        }

        self.grid.write_to(writer)?;
        Ok(())
    }

    pub fn read_from(reader: &mut dyn Read, metadata: &Metadata) -> Result<Self, std::io::Error> {
        let number_of_points = reader.read_u32::<BigEndian>()?;

        let has_over_flow = reader.read_u8()? != 0;
        let overflow = if has_over_flow {
            Some(reader.read_u32::<BigEndian>()?)
        } else {
            None
        };

        let grid = Bits::read_from(reader, metadata.number_of_sub_grid_cells() as usize)?;

        Ok(Self {
            number_of_points,
            overflow,
            grid,
        })
    }
}
