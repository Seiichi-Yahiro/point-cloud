struct VPUniform {
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
    view_proj: mat4x4<f32>,
}

// View Matrix
// RightX      RightY      RightZ      0
// UpX         UpY         UpZ         0
// LookX       LookY       LookZ       0
// PosX        PosY        PosZ        1

@group(0) @binding(0)
var<uniform> vp: VPUniform;

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

struct Cell {
    hierarchy: u32,
    x: i32,
    y: i32,
    z: i32,
}

@group(2) @binding(0)
var<uniform> cell: Cell;

struct VisibleCells {
    len: u32,
    cells: array<Cell>
}

@group(3) @binding(0)
var<storage, read> visible_cells: VisibleCells;

struct InstanceInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>
}

struct VertexInput {
    @builtin(vertex_index) index: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) splat_pos: vec2<f32>,
    @location(2) @interpolate(flat) splat_radius: f32,
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

fn cell_index(position: vec3<f32>, cell_size: f32) -> vec3<i32> {
    return vec3<i32>(floor(position / cell_size));
}

fn get_splat_radius(position: vec3<f32>) -> f32 {
    var spacing = metadata.hierarchies[cell.hierarchy].spacing;
    var current_hierarchy = cell.hierarchy + 1;
    
    if current_hierarchy == metadata.number_of_hierarchies {
        return spacing;
    }
    
    var hierarchy = metadata.hierarchies[current_hierarchy];
    var index = cell_index(position, hierarchy.cell_size);
    
    var i = 0u;
    
    while i < visible_cells.len {
        let visible_cell = visible_cells.cells[i];
        
        if visible_cell.hierarchy == current_hierarchy {
            if all(index == vec3(visible_cell.x, visible_cell.y, visible_cell.z)) {
                spacing = hierarchy.spacing;
                
                current_hierarchy = current_hierarchy + 1;
                hierarchy = metadata.hierarchies[current_hierarchy];
                index = cell_index(position, hierarchy.cell_size);
            }
        } else if visible_cell.hierarchy > current_hierarchy {
            return spacing;
        }
        
        i = i + 1;
    }
   
   return spacing;
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;

    let view_t = transpose(vp.view);

    let cam_right = view_t[0].xyz;
    let cam_up = view_t[1].xyz;

    let radius = get_splat_radius(instance.position);
    let local_splat_position = get_splat_position(vertex.index, radius);
    let bill_board_offset = cam_right * local_splat_position.x + cam_up * local_splat_position.y;
    let billboard_position = vec4<f32>(instance.position + bill_board_offset, 1.0);

    out.clip_position = vp.view_proj * billboard_position;
    out.color = instance.color;
    out.splat_pos = local_splat_position;
    out.splat_radius = radius;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (length(in.splat_pos) > in.splat_radius) {
        discard;
    }
    return vec4<f32>(in.color);
}