struct VPUniform {
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
}

// View Matrix
// RightX      RightY      RightZ      0
// UpX         UpY         UpZ         0
// LookX       LookY       LookZ       0
// PosX        PosY        PosZ        1

@group(0) @binding(0)
var<uniform> vp: VPUniform;

struct InstanceInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>
}

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) splat_pos: vec2<f32>,
}

const RADIUS = 0.1;

fn get_splat_position(index: u32) -> vec2<f32> {
    var POSITIONS = array<vec2<f32>, 4>(
        vec2<f32>(RADIUS, -RADIUS),
        vec2<f32>(RADIUS, RADIUS),
        vec2<f32>(-RADIUS, -RADIUS),
        vec2<f32>(-RADIUS, RADIUS)
    );

    return POSITIONS[index];
}

@vertex
fn vs_main(in: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;

    let view_t = transpose(vp.view);

    let cam_right = view_t[0].xyz;
    let cam_up = view_t[1].xyz;

    let local_splat_position = get_splat_position(in.vertex_index);
    let bill_board_offset = cam_right * local_splat_position.x + cam_up * local_splat_position.y;
    let billboard_position = vec4<f32>(instance.position + bill_board_offset, 1.0);

    out.clip_position = vp.projection * vp.view * billboard_position;
    out.color = instance.color;
    out.splat_pos = local_splat_position;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (length(in.splat_pos) > RADIUS) {
        discard;
    }
    return vec4<f32>(in.color);
}