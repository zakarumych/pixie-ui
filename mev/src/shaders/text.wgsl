struct Globals {
    width: u32,
    height: u32,
}

struct GlyphInstance {
    geom: vec4<f32>,
    atlas_uv: vec4<f32>,
    flags: vec4<u32>,
    color0: vec4<f32>,
    color1: vec4<f32>,
    grad: vec4<f32>,
    uvparam: vec4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<storage, read> instances: array<GlyphInstance>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var atlas_samp: sampler;
@group(0) @binding(4) var tex: texture_2d<f32>;
@group(0) @binding(5) var samp: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) instance_index: u32,
    @location(1) pixel_pos: vec2<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let inst = instances[instance_index];

    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let c = corners[vertex_index];

    let lt = inst.geom.xy;
    let rb = inst.geom.zw;
    let px = mix(lt, rb, c);

    let uv0 = inst.atlas_uv.xy;
    let uv1 = inst.atlas_uv.zw;
    let uv = mix(uv0, uv1, c);

    var out: VertexOutput;
    out.pixel_pos = px;
    out.uv = uv;
    out.instance_index = instance_index;

    let clip_x = px.x / f32(globals.width) * 2.0 - 1.0;
    let clip_y = 1.0 - px.y / f32(globals.height) * 2.0;
    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let inst = instances[in.instance_index];
    let coverage = textureSample(atlas, atlas_samp, in.uv).r;

    let kind = inst.flags.x;
    var color: vec4<f32>;
    if kind == 0u {
        color = inst.color0;
    } else if kind == 1u {
        let start = inst.grad.xy;
        let end = inst.grad.zw;
        let axis = end - start;
        let len2 = dot(axis, axis);
        var t: f32 = 0.0;
        if len2 > 0.0 {
            t = clamp(dot(in.pixel_pos - start, axis) / len2, 0.0, 1.0);
        }
        color = mix(inst.color0, inst.color1, t);
    } else {
        let uv2 = in.pixel_pos * inst.uvparam.xy + inst.uvparam.zw;
        color = textureSample(tex, samp, uv2);
    }

    return color * coverage;
}
