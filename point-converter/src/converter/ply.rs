use std::fs::File;
use std::io::{BufRead, BufReader, Error};
use std::path::Path;

use ply_rs::parser::Parser;
use ply_rs::ply::{Encoding, Header};

use crate::converter::BatchedPointReader;
use crate::point::Point;

pub struct BatchedPlyPointReader {
    buf_reader: BufReader<File>,
    parser: Parser<Point>,
    header: Header,
    read_points: u64,
}

impl BatchedPlyPointReader {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let file = File::open(path).unwrap();
        let mut buf_reader = BufReader::new(file);

        let parser = Parser::new();
        let header = parser.read_header(&mut buf_reader).unwrap();

        Self {
            buf_reader,
            parser,
            header,
            read_points: 0,
        }
    }
}

impl BatchedPointReader for BatchedPlyPointReader {
    fn get_batch(&mut self, size: usize) -> Result<Vec<Point>, Error> {
        let element = self.header.elements.get("vertex").unwrap();
        let point_count = self.remaining_points().min(size as u64);

        let mut batch = Vec::with_capacity(point_count as usize);

        match self.header.encoding {
            Encoding::Ascii => {
                let mut line_str = String::new();
                for _ in 0..point_count {
                    line_str.clear();
                    self.buf_reader.read_line(&mut line_str)?;
                    self.parser.read_ascii_element(&line_str, element)?;
                    self.read_points += 1;
                }
            }
            Encoding::BinaryBigEndian => {
                for _ in 0..point_count {
                    let point = self
                        .parser
                        .read_big_endian_element(&mut self.buf_reader, element)?;
                    batch.push(point);
                    self.read_points += 1;
                }
            }
            Encoding::BinaryLittleEndian => {
                for _ in 0..point_count {
                    let point = self
                        .parser
                        .read_little_endian_element(&mut self.buf_reader, element)?;
                    batch.push(point);
                    self.read_points += 1;
                }
            }
        }

        Ok(batch)
    }

    fn total_points(&self) -> u64 {
        self.header.elements.get("vertex").unwrap().count as u64
    }

    fn remaining_points(&self) -> u64 {
        self.total_points() - self.read_points
    }
}
