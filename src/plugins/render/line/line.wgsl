struct VPUniform {
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
    view_proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> vp: VPUniform;

@group(0) @binding(1)
var<uniform> viewport: vec2<u32>; // width, height

struct InstanceInput {
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
    @location(2) color: vec4<f32>
}

struct VertexInput {
    @builtin(vertex_index) index: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

const LINE_THICKNESS = 50.0;

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    out.color = instance.color;

    let resolution = vec2<f32>(viewport);

    let start_clip: vec4<f32> = vp.view_proj * vec4(instance.start, 1.0);
    let end_clip: vec4<f32> = vp.view_proj * vec4(instance.end, 1.0);

    let start_ndc: vec2<f32> = start_clip.xy / start_clip.w;
    let end_ndc: vec2<f32> = end_clip.xy / end_clip.w;

    let start_screen: vec2<f32> = resolution * 0.5 * (start_ndc + 1.0);
    let end_screen: vec2<f32> = resolution * 0.5 * (end_ndc + 1.0);

    let dir = normalize(end_screen - start_screen);
    let dir_normal = vec2(-dir.y, dir.x);

    let clip: vec4<f32> = alternate_start_end_4(vertex.index, start_clip, end_clip);
    let screen: vec2<f32> = alternate_start_end_2(vertex.index, start_screen, end_screen);

    var half_line_width = LINE_THICKNESS / (clip.w * 2.0);
    if (half_line_width < 0.5) {
        half_line_width = 0.5;
    }

    let screen_offset = dir_normal * half_line_width * f32(alternateSign(vertex.index));
    let screen_pos = screen + screen_offset;
    let ndc_pos = (2.0 * screen_pos) / resolution - 1.0;
    let clip_pos = ndc_pos * clip.w;

    out.clip_position = vec4(clip_pos, clip.z, clip.w);

    return out;
}

fn alternate_start_end_4(index: u32, start: vec4<f32>, end: vec4<f32>) -> vec4<f32> {
    if (index <= 1u) {
        return start;
    } else {
        return end;
    }
}

fn alternate_start_end_2(index: u32, start: vec2<f32>, end: vec2<f32>) -> vec2<f32> {
    if (index <= 1u) {
        return start;
    } else {
        return end;
    }
}

fn alternateSign(index: u32) -> i32 {
    if (index % 2u == 0u) {
        return 1;
    } else {
        return -1;
    }
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}