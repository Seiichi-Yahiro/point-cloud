use std::f32::consts::PI;

use glam::Vec3;
use itertools::Itertools;

use crate::plugins::render::line::Line;

pub fn line_strip(color: [u8; 4], points: &[Vec3]) -> Vec<Line> {
    points
        .iter()
        .copied()
        .tuple_windows::<(_, _)>()
        .map(|(start, end)| Line { start, end, color })
        .collect()
}

pub fn line_box(color: [u8; 4], pos: Vec3, half_extends: Vec3) -> Vec<Line> {
    let near_top_left = pos + Vec3::new(-half_extends.x, -half_extends.y, half_extends.z);
    let near_top_right = pos + Vec3::new(half_extends.x, -half_extends.y, half_extends.z);
    let near_bottom_left = pos + Vec3::new(-half_extends.x, -half_extends.y, -half_extends.z);
    let near_bottom_right = pos + Vec3::new(half_extends.x, -half_extends.y, -half_extends.z);

    let far_top_left = pos + Vec3::new(-half_extends.x, half_extends.y, half_extends.z);
    let far_top_right = pos + Vec3::new(half_extends.x, half_extends.y, half_extends.z);
    let far_bottom_left = pos + Vec3::new(-half_extends.x, half_extends.y, -half_extends.z);
    let far_bottom_right = pos + Vec3::new(half_extends.x, half_extends.y, -half_extends.z);

    [
        (near_top_left, near_top_right),
        (near_top_right, near_bottom_right),
        (near_bottom_right, near_bottom_left),
        (near_bottom_left, near_top_left),
        //
        (far_top_left, far_top_right),
        (far_top_right, far_bottom_right),
        (far_bottom_right, far_bottom_left),
        (far_bottom_left, far_top_left),
        //
        (near_top_left, far_top_left),
        (near_top_right, far_top_right),
        (near_bottom_right, far_bottom_right),
        (near_bottom_left, far_bottom_left),
    ]
    .into_iter()
    .map(|(start, end)| Line { start, end, color })
    .collect()
}

pub fn line_sphere(
    color: [u8; 4],
    pos: Vec3,
    radius: f32,
    number_of_points: u32,
    number_of_rings: u32,
) -> Vec<Line> {
    let mut xzs = Vec::new();
    let mut yzs = Vec::new();
    let mut rings = Vec::new();

    for ring in 0..number_of_rings {
        let mut xys = Vec::new();
        let z_offset = -radius + 2.0 * radius * (ring + 1) as f32 / (number_of_rings + 1) as f32;
        let adjusted_radius = (radius.powi(2) - z_offset.powi(2)).sqrt();

        for i in 0..number_of_points {
            let i = i as f32;
            let number_of_points = number_of_points as f32;

            let angle = 2.0 * PI * i / number_of_points;
            let (sin, cos) = angle.sin_cos();

            let xy = Vec3::new(
                pos.x + adjusted_radius * cos,
                pos.y + adjusted_radius * sin,
                pos.z + z_offset,
            );

            xys.push(xy);
        }

        rings.extend(
            xys.into_iter()
                .circular_tuple_windows::<(_, _)>()
                .map(|(start, end)| Line { start, end, color }),
        );
    }

    for i in 0..number_of_points {
        let angle = 2.0 * PI * i as f32 / number_of_points as f32;
        let (sin, cos) = angle.sin_cos();
        let xz = Vec3::new(pos.x + radius * cos, pos.y, pos.z + radius * sin);
        let yz = Vec3::new(pos.x, pos.y + radius * cos, pos.z + radius * sin);

        xzs.push(xz);
        yzs.push(yz);
    }

    let xzs = xzs
        .into_iter()
        .circular_tuple_windows::<(_, _)>()
        .map(|(start, end)| Line { start, end, color });

    let yzs = yzs
        .into_iter()
        .circular_tuple_windows::<(_, _)>()
        .map(|(start, end)| Line { start, end, color });

    rings.extend(xzs);
    rings.extend(yzs);

    rings
}
