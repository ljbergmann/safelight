// Draws a preview texture into the live view's surface, fitted and centred.
// The texture holds display-encoded sRGB bytes and the surface stores them as
// they are, so filtering mixes encoded values, as the desktop's viewport does.

struct Fit {
    // The fitted image's size as a fraction of the surface's, per axis.
    scale: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear: sampler;
@group(0) @binding(2) var<uniform> fit: Fit;

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Four vertices as a triangle strip: the image's corners, top-left first.
@vertex
fn vs(@builtin(vertex_index) index: u32) -> Varyings {
    let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let ndc = (corner * 2.0 - 1.0) * fit.scale;
    var out: Varyings;
    out.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = corner;
    return out;
}

@fragment
fn fs(in: Varyings) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(source, linear, in.uv).rgb, 1.0);
}
