// Shaders untuk BubbleDesk (spec §11)
// Mengandung pass untuk: background wallpaper, instanced bubbles dengan SDF, dan debug overlay.

struct ScreenUniform {
    screen_size: vec2<f32>,
    background_dim: f32,
    time: f32,
};

@group(0) @binding(0)
var<uniform> uniforms: ScreenUniform;

// ----------------------------------------------------------------------------
// PASS 1: Wallpaper Latar Belakang
// ----------------------------------------------------------------------------

@group(1) @binding(0)
var t_background: texture_2d<f32>;
@group(1) @binding(1)
var s_background: sampler;

struct BackgroundVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_background(@builtin(vertex_index) vertex_index: u32) -> BackgroundVertexOutput {
    // Segitiga besar yang menutupi seluruh layar (fullscreen triangle)
    var out: BackgroundVertexOutput;
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    let p = pos[vertex_index];
    out.clip_position = vec4<f32>(p, 0.0, 1.0);
    // UV: (0,0) di kiri-atas, (1,1) di kanan-bawah
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

@fragment
fn fs_background(in: BackgroundVertexOutput) -> @location(0) vec4<f32> {
    let sample = textureSample(t_background, s_background, in.uv);
    // Terapkan background_dim (spec §11: dimming dikerjakan di shader)
    let dim = clamp(uniforms.background_dim, 0.0, 1.0);
    let rgb = sample.rgb * (1.0 - dim);
    return vec4<f32>(rgb, 1.0);
}

// ----------------------------------------------------------------------------
// PASS 2: Instanced Bubbles dengan SDF + Atlas Ikon
// ----------------------------------------------------------------------------

@group(1) @binding(0)
var t_atlas_0: texture_2d<f32>;
@group(1) @binding(1)
var t_atlas_1: texture_2d<f32>;
@group(1) @binding(2)
var s_atlas: sampler;

struct BubbleVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
    @location(1) eff_radius: f32,
    @location(2) ring_color: vec4<f32>,
    @location(3) ring_width: f32,
    @location(4) glow: f32,
    @location(5) icon_uv: vec4<f32>,
    @location(6) @interpolate(flat) icon_page: i32,
    @location(7) @interpolate(flat) flags: u32,
};

@vertex
fn vs_bubble(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) visual_scale: f32,
    @location(3) ring_color: vec4<f32>,
    @location(4) ring_width: f32,
    @location(5) glow: f32,
    @location(6) flags: u32,
    @location(7) icon_uv: vec4<f32>,
    @location(8) icon_page: i32,
) -> BubbleVertexOutput {
    var out: BubbleVertexOutput;

    // Unit quad (6 vertex)
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0)
    );

    let corner = corners[vertex_index];
    let eff_radius = radius * visual_scale;
    let extent = eff_radius + ring_width + glow + 2.0;
    let world_pos = center + corner * extent;

    // Konversi koordinat piksel layar ke Normalized Device Coordinates (NDC)
    let ndc_x = (world_pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (world_pos.y / uniforms.screen_size.y) * 2.0;

    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.local_pos = corner * extent;
    out.eff_radius = eff_radius;
    out.ring_color = ring_color;
    out.ring_width = ring_width;
    out.glow = glow;
    out.icon_uv = icon_uv;
    out.icon_page = icon_page;
    out.flags = flags;

    return out;
}

@fragment
fn fs_bubble(in: BubbleVertexOutput) -> @location(0) vec4<f32> {
    let dist = length(in.local_pos) - in.eff_radius;
    let max_dist = max(in.glow + 2.0, 1.5);
    if (dist > max_dist) {
        discard;
    }

    let is_hovered = (in.flags & 1u) != 0u;
    let is_error = (in.flags & 2u) != 0u;

    // Warna cincin: merah saat error peluncuran, atau lebih cerah saat hover
    var active_ring_color = in.ring_color;
    if (is_error) {
        active_ring_color = vec4<f32>(0.95, 0.28, 0.28, 1.0);
    } else if (is_hovered) {
        active_ring_color = vec4<f32>(
            min(in.ring_color.rgb * 1.18 + vec3<f32>(0.08), vec3<f32>(1.0)),
            1.0
        );
    }

    // Tepi anti-alias lewat smoothstep pada SDF (spec §11)
    let edge_alpha = smoothstep(1.0, -1.0, dist);

    // Koordinat ternormalisasi di dalam bubble (-1.0 s.d 1.0)
    let norm_coord = in.local_pos / in.eff_radius;

    // Isi semi-transparan ala kaca (gradasi halus & highlight kiri-atas, spec §11)
    let glass_base = vec3<f32>(0.92, 0.95, 1.0);
    let dist_from_center = length(norm_coord);
    let hover_glass_boost = select(0.0, 0.08, is_hovered);
    let glass_alpha = 0.16 + 0.08 * (1.0 - dist_from_center) + hover_glass_boost;

    // Sampling ikon di tengah dengan ukuran 60% dari diameter bubble (spec §11)
    let icon_half = 0.60;
    let in_icon = in.icon_page >= 0
        && abs(norm_coord.x) <= icon_half
        && abs(norm_coord.y) <= icon_half;
    var icon_sample = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (in_icon) {
        let local_uv = (norm_coord / icon_half) * 0.5 + vec2<f32>(0.5, 0.5);
        let atlas_uv = mix(in.icon_uv.xy, in.icon_uv.zw, local_uv);
        if (in.icon_page == 0) {
            icon_sample = textureSampleLevel(t_atlas_0, s_atlas, atlas_uv, 0.0);
        } else if (in.icon_page == 1) {
            icon_sample = textureSampleLevel(t_atlas_1, s_atlas, atlas_uv, 0.0);
        }
    }

    // Komposisi straight-alpha ikon di atas badan kaca
    let icon_a = icon_sample.a;
    let combined_alpha = icon_a + glass_alpha * (1.0 - icon_a);
    var interior_rgb = glass_base;
    if (combined_alpha > 0.001) {
        interior_rgb = (icon_sample.rgb * icon_a + glass_base * glass_alpha * (1.0 - icon_a)) / combined_alpha;
    }

    // Highlight spekular di kiri-atas
    let hl_center = vec2<f32>(-0.35, -0.35);
    let hl_dist = length(norm_coord - hl_center);
    let hl_strength = select(0.36, 0.48, is_hovered);
    let highlight = smoothstep(0.40, 0.0, hl_dist) * hl_strength;

    // Cincin tipis berwarna per jenis
    let ring_dist = abs(dist + in.ring_width * 0.5) - in.ring_width * 0.5;
    let ring_alpha = smoothstep(1.0, -1.0, ring_dist) * active_ring_color.a * edge_alpha;

    // Glow luar lembut (aktif saat hover, pop, atau error)
    var glow_alpha = 0.0;
    if (in.glow > 0.05) {
        let glow_t = clamp(1.0 - max(dist, 0.0) / in.glow, 0.0, 1.0);
        glow_alpha = glow_t * glow_t * 0.55;
    }

    // Komposisi visual akhir: isi (kaca + ikon) + highlight + cincin + glow luar
    let body_rgb = interior_rgb + vec3<f32>(highlight * 0.55);
    let body_alpha = clamp((combined_alpha + highlight * 0.25) * edge_alpha + ring_alpha, 0.0, 1.0);
    var final_rgb = mix(body_rgb, active_ring_color.rgb, ring_alpha);
    let final_alpha = clamp(body_alpha + glow_alpha * (1.0 - body_alpha), 0.0, 1.0);
    if (final_alpha > 0.001 && glow_alpha > 0.001) {
        let glow_contrib = glow_alpha * (1.0 - body_alpha);
        final_rgb = (final_rgb * body_alpha + active_ring_color.rgb * glow_contrib) / final_alpha;
    }

    return vec4<f32>(final_rgb, final_alpha);
}

// ----------------------------------------------------------------------------
// PASS 3: Debug Overlay (spec §14)
// ----------------------------------------------------------------------------

@group(1) @binding(0)
var t_overlay: texture_2d<f32>;
@group(1) @binding(1)
var s_overlay: sampler;

struct OverlayVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_overlay(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect_pos: vec2<f32>,
    @location(1) rect_size: vec2<f32>,
) -> OverlayVertexOutput {
    var out: OverlayVertexOutput;
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0)
    );
    let uv = corners[vertex_index];
    let world_pos = rect_pos + uv * rect_size;

    let ndc_x = (world_pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (world_pos.y / uniforms.screen_size.y) * 2.0;

    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_overlay(in: OverlayVertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_overlay, s_overlay, in.uv);
}

// ----------------------------------------------------------------------------
// PASS 4: Latar Pill Label & Teks Glyph Atlas R8 (spec §11)
// ----------------------------------------------------------------------------

struct LabelBgOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_px: vec2<f32>,
    @location(1) half_size: vec2<f32>,
};

@vertex
fn vs_label_bg(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect_pos: vec2<f32>,
    @location(1) rect_size: vec2<f32>,
) -> LabelBgOutput {
    var out: LabelBgOutput;
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0)
    );
    let uv = corners[vertex_index];
    let world_pos = rect_pos + uv * rect_size;

    let ndc_x = (world_pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (world_pos.y / uniforms.screen_size.y) * 2.0;

    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.half_size = rect_size * 0.5;
    out.local_px = (uv - vec2<f32>(0.5, 0.5)) * rect_size;
    return out;
}

@fragment
fn fs_label_bg(in: LabelBgOutput) -> @location(0) vec4<f32> {
    // Rounded-box SDF untuk latar belakang pill label
    let radius = min(in.half_size.y, 6.0);
    let q = abs(in.local_px) - (in.half_size - vec2<f32>(radius));
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    let alpha = smoothstep(0.8, -0.8, d) * 0.78;
    if (alpha <= 0.01) {
        discard;
    }
    return vec4<f32>(0.05, 0.07, 0.10, alpha);
}

@group(1) @binding(0)
var t_glyph: texture_2d<f32>;
@group(1) @binding(1)
var s_glyph: sampler;

struct GlyphVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_text(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv_min: vec2<f32>,
    @location(3) uv_max: vec2<f32>,
    @location(4) color: vec4<f32>,
) -> GlyphVertexOutput {
    var out: GlyphVertexOutput;
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0)
    );
    let c = corners[vertex_index];
    let world_pos = pos + c * size;

    let ndc_x = (world_pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (world_pos.y / uniforms.screen_size.y) * 2.0;

    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = mix(uv_min, uv_max, c);
    out.color = color;
    return out;
}

@fragment
fn fs_text(in: GlyphVertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(t_glyph, s_glyph, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}
