use glam::Vec3;

const SQRT_3: f32 = 1.73205080757;

pub trait HexWorldIndex {
    fn to_world(&self, cell_radius: f32) -> Vec3;
    fn from_world(pos: Vec3, cell_radius: f32) -> Self;
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Hash)]
pub struct OffsetIndex {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl OffsetIndex {
    pub fn to_axial(self) -> AxialIndex {
        AxialIndex {
            q: self.x - (self.y - (self.y & 1)) / 2,
            r: self.y,
            h: self.z,
        }
    }
}

impl HexWorldIndex for OffsetIndex {
    fn to_world(&self, cell_radius: f32) -> Vec3 {
        self.to_axial().to_world(cell_radius)
    }

    fn from_world(pos: Vec3, cell_radius: f32) -> Self {
        AxialIndex::from_world(pos, cell_radius).to_offset()
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Hash)]
pub struct AxialIndex {
    pub q: i32,
    pub r: i32,
    pub h: i32,
}

impl AxialIndex {
    pub fn to_offset(self) -> OffsetIndex {
        OffsetIndex {
            x: self.q + (self.r - (self.r & 1)) / 2,
            y: self.r,
            z: self.h,
        }
    }
}

impl HexWorldIndex for AxialIndex {
    fn to_world(&self, cell_radius: f32) -> Vec3 {
        let q = self.q as f32;
        let r = self.r as f32;
        let h = self.h as f32;

        let x = cell_radius * (SQRT_3 * q + SQRT_3 / 2.0 * r);
        let y = cell_radius * 3.0 / 2.0 * r;
        let z = h * cell_radius;

        Vec3::new(x, y, z)
    }

    fn from_world(pos: Vec3, cell_radius: f32) -> Self {
        // Convert to their coordinate system
        let x = pos.x / (cell_radius * SQRT_3);
        let y = pos.y / (-cell_radius * SQRT_3);
        // Algorithm from Charles Chambers
        // with modifications and comments by Chris Cox 2023
        // <https://gitlab.com/chriscox/hex-coordinates>
        let t = SQRT_3 * y + 1.0; // scaled y, plus phase
        let temp1 = (t + x).floor(); // (y+x) diagonal, this calc needs floor
        let temp2 = t - x; // (y-x) diagonal, no floor needed
        let temp3 = 2.0 * x + 1.0; // scaled horizontal, no floor needed, needs +1 to get correct phase
        let qf = (temp1 + temp3) / 3.0; // pseudo x with fraction
        let rf = (temp1 + temp2) / 3.0; // pseudo y with fraction

        let q = qf.floor() as i32; // pseudo x, quantized and thus requires floor
        let r = -(rf.floor() as i32); // pseudo y, quantized and thus requires floor
        let h = (pos.z / cell_radius) as i32;
        Self { q, r, h }
    }
}
