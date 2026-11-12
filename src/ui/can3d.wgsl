// A 500 ml drink can, drawn as pixel art that is still real 3D: one ray is traced for each cell
// of a coarse grid, against the can's actual cross-section spun around its axis. The cross-section is in millimetres (radius, height) and comes from a
// can maker's published 211/202 spec: 168.0 tall, 66.3 body, 52.4 neck, 48.3 stand. The curves
// between those measurements are estimated. scripts/can-blender.py builds a Blender model from
// this same table, so the two can be compared. The stay-on tab is separate geometry hinged at its
// rivet, and the mouth it opens is carved out of the lid, so the can really opens.
struct Can {
    rect: vec4<f32>, clip: vec4<f32>, fade: vec4<u32>, viewport: vec4<f32>,
    base: vec4<f32>, main: vec4<f32>, high: vec4<f32>, second: vec4<f32>,
    // spin, roll, rarity (0 Common to 4 Legendary), whether to encode sRGB here
    pose: vec4<f32>,
    // opacity, camera pitch, how far the tab is levered up (radians), how deep the mouth is
    // pushed in (mm)
    extra: vec4<f32>,
    // zoom, whether the can is bare aluminium with no print
    more: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Can;

const POINTS = 19;
var<private> PROFILE: array<vec2<f32>, 19> = array<vec2<f32>, 19>(
    vec2<f32>(0.0, 10.5), vec2<f32>(14.0, 9.0), vec2<f32>(21.0, 4.5), vec2<f32>(24.15, 0.0),
    vec2<f32>(26.5, 0.9), vec2<f32>(30.5, 5.5), vec2<f32>(33.15, 11.5), vec2<f32>(33.15, 150.0),
    vec2<f32>(32.4, 153.5), vec2<f32>(29.5, 158.5), vec2<f32>(26.9, 162.0), vec2<f32>(26.2, 163.8),
    vec2<f32>(27.0, 165.5), vec2<f32>(27.0, 168.0), vec2<f32>(26.0, 168.0), vec2<f32>(25.4, 161.5),
    vec2<f32>(23.9, 160.6), vec2<f32>(23.1, 162.8), vec2<f32>(0.0, 162.8),
);
const HEIGHT = 168.0;
const LABEL_BOTTOM = 11.5;
const LABEL_TOP = 150.0;
const LID = 162.8;
const EYE_DISTANCE = 620.0;
const FILL = 0.9;
const TAU = 6.2831853;

struct Vertex { @builtin(position) position: vec4<f32>, @location(0) p: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) index: u32) -> Vertex {
    let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let p = u.rect.xy + corner * u.rect.zw;
    let local = p - u.viewport.zw;
    var out: Vertex;
    out.position = vec4<f32>(local.x / u.viewport.x * 2.0 - 1.0, 1.0 - local.y / u.viewport.y * 2.0, 0.0, 1.0);
    out.p = p;
    return out;
}

// Signed distance to the cross-section polygon, negative inside the metal's outline.
fn section(p: vec2<f32>) -> f32 {
    var d = dot(p - PROFILE[0], p - PROFILE[0]);
    var s = 1.0;
    var j = POINTS - 1;
    for (var i = 0; i < POINTS; i++) {
        let e = PROFILE[j] - PROFILE[i];
        let w = p - PROFILE[i];
        let b = w - e * clamp(dot(w, e) / dot(e, e), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c = vec3<bool>((p.y >= PROFILE[i].y), (p.y < PROFILE[j].y), (e.x * w.y > e.y * w.x));
        if all(c) || !any(c) { s = -s; }
        j = i;
    }
    return s * sqrt(d);
}
fn spun(p: vec3<f32>) -> vec3<f32> {
    let c = cos(u.pose.x); let s = sin(u.pose.x);
    return vec3<f32>(c * p.x + s * p.z, p.y, -s * p.x + c * p.z);
}
// The scored panel the tab pushes in, as a distance in the lid's plane.
fn mouth(q: vec3<f32>) -> f32 {
    return (length(vec2<f32>(q.x / 7.2, (q.z - 11.0) / 8.6)) - 1.0) * 7.2;
}
// The tab: a plate with a finger ring, levered about the rivet that holds it to the lid.
fn tab(q: vec3<f32>) -> f32 {
    let v = q - vec3<f32>(0.0, LID + 0.9, 0.0);
    let c = cos(u.extra.z); let s = sin(u.extra.z);
    let l = vec3<f32>(v.x, v.y * c + v.z * s, -v.y * s + v.z * c);
    var plate = length(vec2<f32>(l.x, max(abs(l.z + 4.5) - 4.8, 0.0))) - 7.2;
    let ring = (length(vec2<f32>(l.x / 4.3, (l.z + 10.5) / 3.8)) - 1.0) * 3.8;
    plate = max(max(plate, -ring), abs(l.y) - 0.55);
    let rivet = max(length(v.xz) - 2.4, abs(v.y) - 1.0);
    return min(plate, rivet);
}
// How near a point is to anywhere the tab can reach, which is cheaper to ask than the tab.
fn tab_reach(q: vec3<f32>) -> f32 { return length(q - vec3<f32>(0.0, LID + 3.0, -4.0)) - 19.0; }
fn scene(p: vec3<f32>) -> f32 {
    let q = spun(p);
    var d = section(vec2<f32>(length(q.xz), q.y));
    if u.extra.w > 0.01 {
        let pit = max(mouth(q), max(q.y - (LID + 3.0), LID - u.extra.w - q.y));
        d = max(d, -pit);
    }
    let reach = tab_reach(q);
    if reach > 1.5 { return min(d, reach); }
    return min(d, tab(q));
}
fn normal(p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(0.06, -0.06);
    return normalize(e.xyy * scene(p + e.xyy) + e.yyx * scene(p + e.yyx) + e.yxy * scene(p + e.yxy) + e.xxx * scene(p + e.xxx));
}

// An original printed design: a pair of lightning bolts wrapped around the can.
fn bolt(a: f32, v: f32) -> f32 {
    let k = fract(a * 2.0 + 0.5) - 0.5;
    let zig = abs(fract(v * 3.0) * 2.0 - 1.0) * 2.0 - 1.0;
    return abs(k - (zig * 0.055 + (v - 0.5) * 0.05)) - mix(0.015, 0.078, v);
}

// The 4×4 ordered-dither threshold for a cell, 0 to 1.
fn bayer(cell: vec2<f32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(u32(cell.y) & 3u) * 4u + (u32(cell.x) & 3u)] + 0.5) / 16.0;
}

fn shown(colour: vec3<f32>, at: vec2<f32>) -> vec4<f32> {
    var c = colour;
    if u.pose.w > 0.5 {
        c = select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), c * 12.92, c <= vec3<f32>(0.0031308));
    }
    let a = clip_alpha(at, u.clip, u.fade) * u.extra.x;
    return vec4<f32>(c * a, a);
}

const INK = vec3<f32>(0.012, 0.012, 0.018);
// How many cells tall the can's box is, whatever its size on screen.
const CELLS = 72.0;

@fragment fn fs(in: Vertex) -> @location(0) vec4<f32> {
    let mm = HEIGHT / (u.rect.w * FILL * u.more.x);
    let centre = u.rect.xy + u.rect.zw * 0.5;
    let cell_size = u.rect.w / CELLS;
    let cell = floor((in.p - u.rect.xy) / cell_size);
    let origin = u.rect.xy + (cell + 0.5) * cell_size;
    var xy = (origin - centre) * vec2<f32>(1.0, -1.0) * mm;
    let cr = cos(u.pose.y); let sr = sin(u.pose.y);
    xy = vec2<f32>(cr * xy.x - sr * xy.y, sr * xy.x + cr * xy.y);

    let up = vec3<f32>(0.0, cos(u.extra.y), -sin(u.extra.y));
    let back = vec3<f32>(0.0, sin(u.extra.y), cos(u.extra.y));
    let middle = vec3<f32>(0.0, HEIGHT * 0.5, 0.0);
    let eye = middle + back * EYE_DISTANCE;
    let target_point = middle + vec3<f32>(xy.x, 0.0, 0.0) + up * xy.y;
    let ray = normalize(target_point - eye);

    var t = EYE_DISTANCE - 125.0;
    var nearest = 1e9;
    var hit = false;
    for (var i = 0; i < 88; i++) {
        let d = scene(eye + ray * t);
        nearest = min(nearest, d);
        if d < 0.03 { hit = true; break; }
        t += d;
        if t > EYE_DISTANCE + 125.0 { break; }
    }
    if !hit {
        // Cells that only just miss the can are its inked outline.
        if nearest > cell_size * mm * 0.85 { discard; }
        return shown(INK, in.p);
    }

    let p = eye + ray * t;
    let n = normal(p);
    let q = spun(p);
    let around_axis = length(q.xz);
    let opened = u.extra.w > 0.01;

    // Bare aluminium by default; the printed sleeve covers the body.
    var albedo = vec3<f32>(0.86, 0.87, 0.89);
    if u.more.y < 0.5 && q.y > LABEL_BOTTOM && q.y < LABEL_TOP && around_axis > 32.5 {
        let v = (q.y - LABEL_BOTTOM) / (LABEL_TOP - LABEL_BOTTOM);
        let around = fract(atan2(q.x, q.z) / TAU + 1.0);
        let px = mm / (TAU * 33.15);
        let d = bolt(around, v);
        albedo = mix(u.base.rgb, u.second.rgb * 0.55, smoothstep(0.1, 0.95, v));
        albedo = mix(albedo, u.main.rgb, smoothstep(px, -px, d));
        albedo = mix(albedo, u.high.rgb, smoothstep(px, -px, d + 0.014));
        // A Legendary's can is gold.
        if u.pose.z >= 3.5 { albedo = mix(albedo, vec3<f32>(1.0, 0.69, 0.24), 0.82); }
    } else if opened && mouth(q) < 0.4 && q.y < LID - 0.25 {
        // Down the mouth: dark metal, then the drink in the flavour's colour.
        albedo = vec3<f32>(0.09, 0.09, 0.1);
        if q.y < LID - u.extra.w + 0.7 { albedo = u.main.rgb * 0.8 + u.high.rgb * 0.12; }
    } else if tab_reach(q) < 1.5 && tab(q) < 0.3 {
        albedo = vec3<f32>(0.94, 0.95, 0.97);
    } else if !opened && abs(q.y - LID) < 0.6 && around_axis < 23.0 {
        // The score line the mouth tears along.
        albedo *= 1.0 - 0.32 * smoothstep(0.75, 0.15, abs(mouth(q)));
    }

    // Flat colour in four steps of light, dithered only where one step meets the next.
    let key = normalize(vec3<f32>(-0.55, 0.45, 0.7));
    let light = 0.3 + 0.85 * clamp(dot(n, key) * 0.5 + 0.5, 0.0, 1.0);
    var colour = albedo * floor(light * 4.0 + (bayer(cell) - 0.5) * 0.7 + 0.5) / 4.0;
    colour = mix(colour, vec3<f32>(1.0), step(0.95, max(dot(n, normalize(key - ray)), 0.0)) * 0.8);
    // Darker where the surface turns away, which rounds the can.
    if dot(n, -ray) < 0.2 { colour *= 0.35; }
    return shown(colour, in.p);
}
