// One pipeline for everything: atlas sprites, and flat ellipses, lines and polygons whose edges are
// antialiased from screen-space derivatives. Output is premultiplied alpha over a transparent surface.

struct View {
    origin: vec2f,   // top-left of this monitor in overlay units
    inv_size: vec2f, // 1 / its size in overlay units
};

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;
@group(1) @binding(0) var<uniform> view: View;

struct VsIn {
    @location(0) pos: vec2f,
    @location(1) uv: vec2f,
    @location(2) color: vec4f,
    @location(3) extra: vec2f, // mode, clip y
};

struct VsOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
    @location(2) @interpolate(flat) mode: f32,
    @location(3) world_y: f32,
    @location(4) @interpolate(flat) clip_y: f32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    let p = (v.pos - view.origin) * view.inv_size;
    o.position = vec4f(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    o.mode = v.extra.x;
    o.world_y = v.pos.y;
    o.clip_y = v.extra.y;
    return o;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4f {
    // derivatives and texture sampling must happen in uniform control flow: work out every mode, then pick
    let tex = textureSample(atlas, atlas_sampler, i.uv);

    let f = length(i.uv) - 1.0;
    let ellipse = clamp(0.5 - f / max(fwidth(f), 1e-5), 0.0, 1.0);

    let d = abs(i.uv) - vec2f(1.0, 1.0);
    let fw = max(fwidth(i.uv), vec2f(1e-5, 1e-5));
    let rect = clamp(0.5 - d.x / fw.x, 0.0, 1.0) * clamp(0.5 - d.y / fw.y, 0.0, 1.0);

    let e = i.uv.x;
    let edge = clamp(e / max(fwidth(e), 1e-5) + 0.5, 0.0, 1.0);

    let pm = vec4f(i.color.rgb * i.color.a, i.color.a);
    let mode = i32(i.mode + 0.5);
    var out = pm;
    if (mode == 0) {
        out = tex * pm;
    } else if (mode == 1) {
        out = pm * ellipse;
    } else if (mode == 2) {
        out = pm * rect;
    } else if (mode == 3) {
        out = pm * edge;
    }
    if (i.world_y > i.clip_y) {
        discard;
    }
    return out;
}

// ---- upscaling a reduced-resolution frame (low and medium quality) onto the surface;
// group 0 then holds that frame instead of the atlas

struct BlitOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn blit_vs(@builtin(vertex_index) idx: u32) -> BlitOut {
    var o: BlitOut;
    let uv = vec2f(f32((idx << 1u) & 2u), f32(idx & 2u));
    o.position = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

@fragment
fn blit_fs(i: BlitOut) -> @location(0) vec4f {
    return textureSample(atlas, atlas_sampler, i.uv);
}
