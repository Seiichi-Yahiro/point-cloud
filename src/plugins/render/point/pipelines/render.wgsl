struct VPUniform {
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    cam_pos: vec3<f32>
}

// View Matrix
// RightX      RightY      RightZ      0
// UpX         UpY         UpZ         0
// LookX       LookY       LookZ       0
// PosX        PosY        PosZ        1

@group(0) @binding(0)
var<uniform> vp: VPUniform;

@group(0) @binding(1)
var<uniform> viewport: vec2<u32>; // width, height

struct Metadata {
    number_of_hierarchies: u32,
    hierarchies: array<Hierarchy>
}

struct Hierarchy {
    cell_size: f32,
    spacing: f32,
}

@group(1) @binding(0)
var<storage, read> metadata: Metadata;

struct InstanceInput {
    @location(0) position: vec3<f32>,
    @location(1) color: u32
}

struct VertexInput {
    @builtin(vertex_index) index: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) splat_pos: vec2<f32>,
    @location(2) @interpolate(flat) splat_radius: f32,
    @location(3) view_pos: vec3<f32>
}

fn get_splat_position(index: u32, radius: f32) -> vec2<f32> {
    var POSITIONS = array<vec2<f32>, 4>(
        vec2<f32>(radius, -radius),
        vec2<f32>(radius, radius),
        vec2<f32>(-radius, -radius),
        vec2<f32>(-radius, radius)
    );

    return POSITIONS[index];
}

fn unpack4x8(packed: u32) -> vec4<u32> {
    return vec4<u32>(
        (packed >> 0) & 0xFF,
        (packed >> 8) & 0xFF,
        (packed >> 16) & 0xFF,
        (packed >> 24) & 0xFF
    );
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;

    let view_t = transpose(vp.view);

    let cam_right = view_t[0].xyz;
    let cam_up = view_t[1].xyz;

    let unpackedColor = unpack4x8(instance.color);

    let hierarchy = unpackedColor.w;
    let radius = metadata.hierarchies[hierarchy].spacing;

    let local_splat_position = get_splat_position(vertex.index, radius);
    let bill_board_offset = cam_right * local_splat_position.x + cam_up * local_splat_position.y;
    let billboard_position = vec4<f32>(instance.position + bill_board_offset, 1.0);

    out.view_pos = (vp.view * billboard_position).xyz;
    out.clip_position = vp.view_proj * billboard_position;
    out.color = vec4<f32>(vec3<f32>(unpackedColor.xyz) / 255.0, 1.0);
    out.splat_pos = local_splat_position;
    out.splat_radius = radius;

    return out;
}

struct FragmentOutput {
    @builtin(frag_depth) depth: f32,
    @location(0) color: vec4<f32>,
}

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;

    if (length(in.splat_pos) > in.splat_radius) {
        discard;
    }
    
    let normalized_splat_pos = in.splat_pos / in.splat_radius;
    let weight = 1.0 - dot(normalized_splat_pos, normalized_splat_pos);
    
    let depth_offset = in.splat_radius * weight;
    
    let pos = vp.projection * vec4(in.view_pos.xy, in.view_pos.z + depth_offset, 1.0);
    let z = pos.z / pos.w;
    
    out.color = vec4<f32>(in.color);
    out.depth = z;
    return out;
}