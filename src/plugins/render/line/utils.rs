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
    let near_top_left = pos + Vec3::new(-half_extends.x, half_extends.y, half_extends.z);
    let near_top_right = pos + Vec3::new(half_extends.x, half_extends.y, half_extends.z);
    let near_bottom_left = pos + Vec3::new(-half_extends.x, -half_extends.y, half_extends.z);
    let near_bottom_right = pos + Vec3::new(half_extends.x, -half_extends.y, half_extends.z);

    let far_top_left = pos + Vec3::new(-half_extends.x, half_extends.y, -half_extends.z);
    let far_top_right = pos + Vec3::new(half_extends.x, half_extends.y, -half_extends.z);
    let far_bottom_left = pos + Vec3::new(-half_extends.x, -half_extends.y, -half_extends.z);
    let far_bottom_right = pos + Vec3::new(half_extends.x, -half_extends.y, -half_extends.z);

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
