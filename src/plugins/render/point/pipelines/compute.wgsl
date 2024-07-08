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

struct LoadedCells {
    len: u32,
    cells: array<Cell>
}

@group(1) @binding(1)
var<storage, read> loaded_cells: LoadedCells;

@group(1) @binding(2)
var<storage, read> frustums_far_distances: array<f32>;

struct FrustumsSettings {
    size_by_distance: u32, // bool
    max_hierarchy: u32
}

@group(1) @binding(3)
var<uniform> frustums_settings: FrustumsSettings;

struct Point {
    position: vec3<f32>,
    color: u32 // vec4<u8>
}

@group(2) @binding(0)
var<storage, read> in: array<Point>;

@group(2) @binding(1)
var<storage, read_write> out: array<Point>;

struct DrawIndirectArgs {
    vertex_count: u32,
    instance_count: atomic<u32>,
    first_vertex: u32,
    first_instance: u32,
}

@group(2) @binding(2)
var<storage, read_write> indirect_buffer: DrawIndirectArgs;

struct Cell {
    hierarchy: u32,
    x: i32,
    y: i32,
    z: i32,
}

@group(2) @binding(3)
var<uniform> cell: Cell;

fn cell_index(position: vec3<f32>, cell_size: f32) -> vec3<i32> {
    return vec3<i32>(floor(position / cell_size));
}

fn get_hierarchy(position: vec3<f32>) -> u32 {
    let own_hierarchy = search_smallest_hierarchy(position, cell.hierarchy);

    if (bool(frustums_settings.size_by_distance)) {
        let distance_to_camera = distance(vp.cam_pos, position);

        for (var i = frustums_settings.max_hierarchy; i > own_hierarchy; i--) {
            if (distance_to_camera < frustums_far_distances[i]) {
                return i;
            }
        }
    }

    return own_hierarchy;
}

fn search_smallest_hierarchy(position: vec3<f32>, start_hierarchy: u32) -> u32 {
    if start_hierarchy >= (metadata.number_of_hierarchies - 1) {
        return metadata.number_of_hierarchies - 1;
    }

    var target_cell: Cell;
    target_cell.hierarchy = start_hierarchy;

    loop {
        target_cell.hierarchy += 1u;

        let cell_size = metadata.hierarchies[target_cell.hierarchy].cell_size;
        let index = cell_index(position, cell_size);

        target_cell.x = index.x;
        target_cell.y = index.y;
        target_cell.z = index.z;

        if (!binary_search(target_cell)) {
            return target_cell.hierarchy - 1u;
        }
    }

    return start_hierarchy; // unreachable but compiler needs this
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

@compute @workgroup_size(128)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let in_index = global_id.x;
   
    if in_index >= arrayLength(&in) {
        return;
    }

    let input = in[in_index];

    let clip = vp.view_proj * vec4(input.position, 1.0);
    let ndc = clip.xyz / clip.w;

    if all(abs(ndc.xy) <= vec2(1.0)) && abs(ndc.z - 0.5) <= 0.5 {
        let hierarchy = get_hierarchy(input.position);
        let unpackedColor = unpack4x8unorm(input.color);

        var output = input;
        output.color = pack4x8unorm(vec4(unpackedColor.xyz, f32(hierarchy) / 255.0));

        let old_index = atomicAdd(&indirect_buffer.instance_count, 1u);
        out[old_index] = output;
    }
}