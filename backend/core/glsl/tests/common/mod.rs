// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A corpus of shaders and a harness that runs each on many pseudo-random inputs and reduces
//! what it writes to a 64-bit fingerprint of the output bits.
//!
//! The fingerprints in `corpus.rs` were produced by the interpreter before it was
//! optimized, and the interpreter is held to them bit for bit: an optimization may change
//! how an expression is evaluated, never its result, including the order of floating-point
//! operations.

#![allow(dead_code)]

use blueice_glsl::ast::SamplerKind;
use blueice_glsl::builtins::Textures;
use blueice_glsl::interp::{Machine, Program};
use blueice_glsl::value::V;

/// Declarations every shader in the corpus shares: its inputs and its outputs.
pub const HEADER: &str = "\
#version 300 es
precision highp float;
precision highp int;
uniform vec4 u0, u1, u2, u3;
uniform mat4 m0;
uniform float f0;
uniform int i0;
uniform sampler2D s0;
out vec4 o0;
out vec4 o1;
";

pub struct Entry {
    pub name: &'static str,
    /// The body after `HEADER` (functions, then `main`).
    pub body: &'static str,
}

pub const CORPUS: &[Entry] = &[
    Entry {
        name: "arithmetic",
        body: "
void main() {
    float a = f0 * 2.0 + 1.0;
    int k = i0 * 3 - 2;
    bool b = a > 1.5 && k < 4 || !(k == 0);
    vec4 v = u0 * u1 + u2 - u3 / (abs(u1) + 0.5);
    v.xy += vec2(a, float(k));
    v.zw *= 0.5;
    uvec2 uu = uvec2(uint(abs(k)), 7u);
    uu = uu * 3u + uu / 2u;
    o0 = v + vec4(b ? 1.0 : 0.0, float(uu.x), float(uu.y % 5u), 0.0);
    o1 = vec4(float(k % 3), float(k << 2), float(k >> 1), float(k ^ 5));
}",
    },
    Entry {
        name: "matrices",
        body: "
void main() {
    mat4 m = m0;
    mat3 n = mat3(m);
    mat2 q = mat2(u0.xy, u1.zw);
    vec4 p = m * u0;
    vec4 r = u1 * m;
    mat4 mm = m * transpose(m);
    float d = determinant(n) + determinant(q);
    mat2 qi = inverse(q + mat2(3.0));
    mat3 sc = n * 0.5 + mat3(1.0);
    o0 = p + r;
    o1 = vec4(mm[1][2], mm[3].x, d, qi[0][1] + sc[2][1]);
}",
    },
    Entry {
        name: "float_builtins",
        body: "
void main() {
    vec4 a = u0, b = abs(u1) + 0.1;
    o0 = vec4(sin(a.x) + cos(a.y) * tan(a.z * 0.3), exp(a.w * 0.5), log(b.x), pow(b.y, a.x * 0.5));
    o1 = vec4(sqrt(b.z), inversesqrt(b.w), floor(a.x) + ceil(a.y), fract(a.z) + sign(a.w));
    o0 += vec4(mod(a.x, 1.5), min(a.y, a.z), max(a.w, b.x), clamp(a.x, -0.5, 0.5));
    o1 += vec4(mix(a.x, a.y, 0.25), step(0.0, a.z), smoothstep(-1.0, 1.0, a.w), radians(a.x) + degrees(a.y));
    o1 += vec4(asin(clamp(a.x * 0.2, -1.0, 1.0)), acos(clamp(a.y * 0.2, -1.0, 1.0)), atan(a.z, b.x), atan(a.w));
    o0 += vec4(sinh(a.x * 0.3), cosh(a.y * 0.3), tanh(a.z), exp2(a.w * 0.3) + log2(b.y));
    o1 += vec4(trunc(a.x), round(a.y), roundEven(a.z), abs(a.w));
}",
    },
    Entry {
        name: "vector_builtins",
        body: "
void main() {
    vec3 a = u0.xyz, b = u1.xyz, c = u2.xyz + vec3(0.1);
    vec3 n = normalize(c);
    float l = length(a), d = distance(a, b), t = dot(a, b);
    vec3 x = cross(a, b);
    vec3 r = reflect(a, n), f = faceforward(a, b, n);
    vec3 rf = refract(normalize(a + vec3(0.2)), n, 0.75);
    vec3 m = mix(a, b, f0 * 0.3);
    vec3 cl = clamp(a, vec3(-0.5), vec3(0.5));
    vec3 st = smoothstep(vec3(-1.0), vec3(1.0), b) + step(vec3(0.0), a);
    o0 = vec4(l, d, t, 1.0) + vec4(x, 0.0) + vec4(r, 0.0);
    o1 = vec4(f + rf, 0.0) + vec4(m, 0.0) + vec4(cl + st, 0.0) + vec4(n, 0.0);
}",
    },
    Entry {
        name: "control_flow",
        body: "
float accumulate(int n, float seed) {
    float s = seed;
    for (int i = 0; i < n; i++) {
        if (i == 3) continue;
        if (i > 8) break;
        s += float(i) * 0.5;
    }
    return s;
}
void bump(inout float v, out float w, float k) {
    v += k;
    w = v * 2.0;
}
int sw(int x) {
    switch (x) {
        case 0: return 10;
        case 1: return 20;
        case 2:
        case 3: return 30;
        default: return 40;
    }
}
void main() {
    int n = 5 + (i0 & 3);
    float acc = accumulate(n, f0);
    float v = acc, w = 0.0;
    bump(v, w, u0.x);
    int k = 0;
    while (k < 6) { k += 2; }
    do { k -= 1; } while (k > 3);
    o0 = vec4(acc, v, w, float(k));
    o1 = vec4(float(sw(abs(i0) % 5)), acc > 3.0 ? 1.0 : -1.0, float(n), 0.0);
}",
    },
    Entry {
        name: "structs_and_arrays",
        body: "
struct Light { vec3 pos; vec4 color; float power[3]; };
Light make(vec3 p, float s) {
    Light l;
    l.pos = p;
    l.color = vec4(p * 0.5, 1.0);
    l.power[0] = s; l.power[1] = s * 2.0; l.power[2] = s * 3.0;
    return l;
}
void main() {
    Light a = make(u0.xyz, f0);
    Light b = make(u1.xyz, f0 + 1.0);
    float table[4] = float[4](1.0, 2.0, 3.0, 4.0);
    table[2] += u2.x;
    vec2 pairs[2] = vec2[2](u0.xy, u1.zw);
    pairs[1].y *= 2.0;
    a.color.w = a.power[2] + b.power[1];
    o0 = vec4(a.pos + b.pos, a.color.w) + vec4(table[0], table[2], table[3], 0.0);
    o1 = a.color + b.color + vec4(pairs[0], pairs[1]);
}",
    },
    Entry {
        name: "swizzles",
        body: "
void main() {
    vec4 v = u0;
    v.zyx = u1.xyz;
    v.wx = u2.yz;
    vec4 w = v.wzyx + v.xxyy * 0.5;
    mat4 m = m0;
    m[1] = u3;
    m[2][1] = f0;
    m[0].zw = u0.xy;
    int i = abs(i0) % 4;
    w[i] += 1.0;
    vec3 s = w.rgb + w.stp;
    o0 = w + vec4(s, 0.0);
    o1 = m[1] + m[2] + vec4(m[0][3], m[3][3], v[i], 0.0);
}",
    },
    Entry {
        name: "lighting",
        body: "
vec3 toLinear(vec3 c) { return pow(c, vec3(2.2)); }
vec3 toSrgb(vec3 c) { return pow(c, vec3(1.0 / 2.2)); }
vec3 shade(vec3 p, vec3 n, vec3 view, vec3 lightPos, vec3 lightColor, float shininess) {
    vec3 l = normalize(lightPos - p);
    float diff = max(dot(n, l), 0.0);
    vec3 h = normalize(l + view);
    float spec = pow(max(dot(n, h), 0.0), shininess);
    return lightColor * (diff + spec * 0.5);
}
void main() {
    vec3 p = u0.xyz, n = normalize(u1.xyz + vec3(0.01)), view = normalize(u2.xyz + vec3(0.01));
    vec3 base = toLinear(abs(u3.rgb) * 0.5);
    vec3 lit = vec3(0.0);
    for (int i = 0; i < 4; i++) {
        float fi = float(i);
        vec3 lp = vec3(fi * 2.0 - 3.0, 4.0, fi - 2.0);
        vec3 lc = vec3(0.5 + 0.1 * fi, 0.8 - 0.1 * fi, 0.6);
        lit += shade(p, n, view, lp, lc, 16.0 + 8.0 * fi);
    }
    vec3 color = base * lit;
    float fog = 1.0 - exp2(-0.05 * 0.05 * dot(p, p) * 1.442695);
    color = mix(color, vec3(0.7, 0.8, 0.9), clamp(fog, 0.0, 1.0));
    color = color / (color + vec3(1.0));
    o0 = vec4(toSrgb(color), 1.0);
    o1 = vec4(lit, fog);
}",
    },
    Entry {
        name: "vertex_like",
        body: "
uniform mat4 modelView;
void main() {
    mat4 inst = m0;
    vec3 position = u0.xyz;
    vec3 normal = normalize(u1.xyz + vec3(0.01));
    vec2 uv = u2.xy;
    mat3 nm = mat3(modelView);
    vec3 vNormal = nm * mat3(inst) * normal;
    vec4 mv = modelView * inst * vec4(position, 1.0);
    vec4 clip = mv * mat4(1.2, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, -1.02, -1.0, 0.0, 0.0, -0.2, 0.0);
    o0 = clip;
    o1 = vec4(vNormal, uv.x + uv.y);
}",
    },
    Entry {
        name: "texturing",
        body: "
void main() {
    vec2 uv = fract(u0.xy);
    vec4 a = texture(s0, uv);
    vec4 b = textureLod(s0, u1.xy * 0.1, 1.0);
    ivec2 sz = textureSize(s0, 0);
    vec4 c = texelFetch(s0, ivec2(abs(i0) % 4, 1), 0);
    o0 = a * 0.5 + b * 0.25 + c * 0.25;
    o1 = vec4(float(sz.x), float(sz.y), a.w, b.x);
}",
    },
    Entry {
        name: "integer_and_bool_vectors",
        body: "
void main() {
    ivec3 a = ivec3(i0, i0 + 1, i0 - 2);
    ivec3 b = ivec3(3, -1, 2);
    ivec3 s = a * b + a / (abs(b) + ivec3(1));
    bvec3 lt = lessThan(vec3(u0.xyz), vec3(u1.xyz));
    bvec3 eq = equal(a, b);
    bool any_lt = any(lt), all_eq = all(eq);
    vec3 pick = vec3(lt.x ? u2.x : u3.x, lt.y ? u2.y : u3.y, lt.z ? u2.z : u3.z);
    o0 = vec4(vec3(s), float(any_lt)) + vec4(pick, float(all_eq));
    o1 = vec4(float(not(lt).x), float(greaterThanEqual(a, b).y), float(notEqual(a, b).z), float(max(a.x, b.x)));
}",
    },
];

/// A texture whose colour is a fixed function of its coordinates.
pub struct TestTextures;

impl Textures for TestTextures {
    fn sample(
        &self,
        _unit: u32,
        _kind: SamplerKind,
        c: [f32; 4],
        lod: Option<f32>,
        bias: f32,
    ) -> [f32; 4] {
        let l = lod.unwrap_or(0.0) + bias;
        [
            c[0].fract(),
            c[1].fract(),
            (c[0] * c[1]).fract(),
            1.0 / (1.0 + l.abs()),
        ]
    }
    fn fetch(&self, _unit: u32, _kind: SamplerKind, c: [i32; 3], lod: i32) -> [f32; 4] {
        [c[0] as f32 * 0.25, c[1] as f32 * 0.5, lod as f32, 1.0]
    }
    fn size(&self, _unit: u32, _kind: SamplerKind, lod: i32) -> [i32; 3] {
        [64 >> lod.clamp(0, 6), 32 >> lod.clamp(0, 5), 1]
    }
}

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// In `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * ((self.next() >> 40) as f32 / (1u64 << 24) as f32)
    }
}

pub fn compile(entry: &Entry) -> Program {
    let source = format!("{HEADER}{}", entry.body);
    Program::compile(&source).unwrap_or_else(|e| panic!("{}: {e}", entry.name))
}

/// Sets every input the corpus shaders share to values drawn from `rng`.
pub fn randomize_inputs(m: &mut Machine<'_>, rng: &mut Rng) {
    for name in ["u0", "u1", "u2", "u3"] {
        let v = [
            rng.range(-2.0, 2.0),
            rng.range(-2.0, 2.0),
            rng.range(-2.0, 2.0),
            rng.range(-2.0, 2.0),
        ];
        m.set_by_name(name, V::vec(&v));
    }
    let mut d = [0.0f32; 16];
    for x in &mut d {
        *x = rng.range(-1.5, 1.5);
    }
    m.set_by_name(
        "m0",
        V::Mat {
            cols: 4,
            rows: 4,
            d,
        },
    );
    let mut mv = [0.0f32; 16];
    for (i, x) in mv.iter_mut().enumerate() {
        *x = if i % 5 == 0 {
            1.0
        } else {
            rng.range(-0.3, 0.3)
        };
    }
    m.set_by_name(
        "modelView",
        V::Mat {
            cols: 4,
            rows: 4,
            d: mv,
        },
    );
    m.set_by_name("f0", V::f(rng.range(0.1, 3.0)));
    m.set_by_name("i0", V::i((rng.next() % 11) as i32 - 5));
    m.set_by_name("s0", V::Sampler(0, SamplerKind::S2D));
}

fn mix(h: u64, bits: u32) -> u64 {
    (h ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn fold(h: u64, v: &V) -> u64 {
    match v {
        V::Vec { n, c, .. } => c[..*n as usize].iter().fold(h, |h, b| mix(h, *b)),
        V::Mat { cols, rows, d } => d[..(*cols * *rows) as usize]
            .iter()
            .fold(h, |h, x| mix(h, x.to_bits())),
        V::Struct(_, fields) => fields.iter().fold(h, fold),
        V::Arr(items) => items.iter().fold(h, fold),
        _ => h,
    }
}

/// How many input sets each shader is run on.
pub const ROUNDS: usize = 64;

/// The shader's outputs on `ROUNDS` pseudo-random inputs, as one number.
pub fn fingerprint(entry: &Entry) -> u64 {
    let program = compile(entry);
    let mut m = Machine::new(&program, &TestTextures).expect("machine");
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ entry.name.len() as u64);
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for _ in 0..ROUNDS {
        randomize_inputs(&mut m, &mut rng);
        m.run_main()
            .unwrap_or_else(|e| panic!("{}: {e}", entry.name));
        for out in ["o0", "o1"] {
            h = fold(h, m.get_by_name(out).expect("an output"));
        }
    }
    h
}
