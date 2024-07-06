struct VPUniform {
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    cam_pos: vec3<f32>
}

@group(0) @binding(0)
var<uniform> vp: VPUniform;

@group(0) @binding(1)
var<uniform> viewport: vec2<u32>; // width, height

struct Point {
    position: vec3<f32>,
    color: u32 // vec4<u8>
}

@group(1) @binding(0)
var<storage, read> in: array<Point>;

@group(1) @binding(1)
var<storage, read_write> out: array<Point>;

struct DrawIndirectArgs {
    vertex_count: u32,
    instance_count: atomic<u32>,
    first_vertex: u32,
    first_instance: u32,
}

@group(1) @binding(2)
var<storage, read_write> indirect_buffer: DrawIndirectArgs;


@compute @workgroup_size(128)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let in_index = global_id.x;
   
    if in_index > arrayLength(&in) {
        return;
    }

    let input = in[in_index];

    let clip = vp.view_proj * vec4(input.position, 1.0);
    let ndc = clip.xyz / clip.w;

    if all(abs(ndc.xy) <= vec2(1.0)) && abs(ndc.z - 0.5) <= 0.5 {
        let old_index = atomicAdd(&indirect_buffer.instance_count, 1u);
        out[old_index] = input;
    }
}