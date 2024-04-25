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

struct Cell {
    hierarchy: u32,
    x: i32,
    y: i32,
    z: i32,
}

@group(2) @binding(0)
var<uniform> cell: Cell;

struct LoadedCells {
    len: u32,
    cells: array<Cell>
}

@group(3) @binding(0)
var<storage, read> loaded_cells: LoadedCells;

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
    
    var target_cell: Cell;
    target_cell.hierarchy = current_hierarchy;
    target_cell.x = index.x;
    target_cell.y = index.y;
    target_cell.z = index.z;
    
    while binary_search(target_cell) {
        spacing = hierarchy.spacing;
                    
        current_hierarchy = current_hierarchy + 1;
        hierarchy = metadata.hierarchies[current_hierarchy];
        index = cell_index(position, hierarchy.cell_size);
        
        target_cell.hierarchy = current_hierarchy;
        target_cell.x = index.x;
        target_cell.y = index.y;
        target_cell.z = index.z;
    }
   
   return spacing;
}

fn binary_search(target_cell: Cell) -> bool {
    var low = 0;
    var high = i32(loaded_cells.len) - 1;

    while (low <= high) {
        var mid = (low + high) / 2;
        var mid_cell = loaded_cells.cells[mid];

        if (
            mid_cell.hierarchy == target_cell.hierarchy 
            && mid_cell.x == target_cell.x 
            && mid_cell.y == target_cell.y 
            && mid_cell.z == target_cell.z
        ) {
            return true;
        } else if (
            mid_cell.hierarchy < target_cell.hierarchy
            || (mid_cell.hierarchy == target_cell.hierarchy && mid_cell.x < target_cell.x) 
            || (mid_cell.hierarchy == target_cell.hierarchy && mid_cell.x == target_cell.x && mid_cell.y < target_cell.y) 
            || (mid_cell.hierarchy == target_cell.hierarchy && mid_cell.x == target_cell.x && mid_cell.y == target_cell.y && mid_cell.z < target_cell.z)
        ) {
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }

    return false;
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

    out.view_pos = (vp.view * billboard_position).xyz;
    out.clip_position = vp.view_proj * billboard_position;
    out.color = instance.color;
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