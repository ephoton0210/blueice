// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! GLSL's built-in functions.

use super::ast::*;
use super::value::*;

/// Where texture reads come from; the pipeline supplies it.
pub trait Textures {
    /// A filtered read. `lod` is explicit when given, else `bias` shifts the
    /// level the implementation would pick.
    fn sample(
        &self,
        unit: u32,
        kind: SamplerKind,
        coord: [f32; 4],
        lod: Option<f32>,
        bias: f32,
    ) -> [f32; 4];
    /// `texelFetch`.
    fn fetch(&self, unit: u32, kind: SamplerKind, coord: [i32; 3], lod: i32) -> [f32; 4];
    /// `textureSize`: width, height, depth.
    fn size(&self, unit: u32, kind: SamplerKind, lod: i32) -> [i32; 3];
}

/// No textures: every read is opaque black... transparent black, as for an
/// incomplete texture.
pub struct NoTextures;
impl Textures for NoTextures {
    fn sample(&self, _: u32, _: SamplerKind, _: [f32; 4], _: Option<f32>, _: f32) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }
    fn fetch(&self, _: u32, _: SamplerKind, _: [i32; 3], _: i32) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }
    fn size(&self, _: u32, _: SamplerKind, _: i32) -> [i32; 3] {
        [1, 1, 1]
    }
}

fn floats(v: &V) -> R<(u8, [f32; 4])> {
    match v {
        V::Vec { k, n, c } => {
            let mut out = [0.0; 4];
            for i in 0..*n as usize {
                out[i] = lane_f32(*k, c[i]);
            }
            Ok((*n, out))
        }
        _ => Err("expected a number or vector".into()),
    }
}

fn from_floats(n: u8, c: [f32; 4]) -> V {
    let mut bits = [0u32; 4];
    for i in 0..n as usize {
        bits[i] = c[i].to_bits();
    }
    V::Vec {
        k: Scalar::Float,
        n,
        c: bits,
    }
}

fn map1(v: &V, f: impl Fn(f32) -> f32) -> R<V> {
    let (n, c) = floats(v)?;
    let mut out = [0.0; 4];
    for i in 0..n as usize {
        out[i] = f(c[i]);
    }
    Ok(from_floats(n, out))
}

/// Component-wise over two arguments, a scalar broadcasting against a vector.
fn map2(a: &V, b: &V, f: impl Fn(f32, f32) -> f32) -> R<V> {
    let ((na, ca), (nb, cb)) = (floats(a)?, floats(b)?);
    let n = na.max(nb);
    let mut out = [0.0; 4];
    for i in 0..n as usize {
        out[i] = f(
            ca[if na == 1 { 0 } else { i }],
            cb[if nb == 1 { 0 } else { i }],
        );
    }
    Ok(from_floats(n, out))
}

fn map3(a: &V, b: &V, c: &V, f: impl Fn(f32, f32, f32) -> f32) -> R<V> {
    let ((na, ca), (nb, cb), (nc, cc)) = (floats(a)?, floats(b)?, floats(c)?);
    let n = na.max(nb).max(nc);
    let mut out = [0.0; 4];
    for i in 0..n as usize {
        out[i] = f(
            ca[if na == 1 { 0 } else { i }],
            cb[if nb == 1 { 0 } else { i }],
            cc[if nc == 1 { 0 } else { i }],
        );
    }
    Ok(from_floats(n, out))
}

fn dot(a: &V, b: &V) -> R<f32> {
    let ((n, ca), (_, cb)) = (floats(a)?, floats(b)?);
    Ok((0..n as usize).map(|i| ca[i] * cb[i]).sum())
}

fn mod_f(x: f32, y: f32) -> f32 {
    x - y * (x / y).floor()
}

fn round_even(x: f32) -> f32 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - x.signum()
    } else {
        r
    }
}

/// Integer-aware `min`/`max`/`abs`/`sign`/`clamp`: for int and uint arguments the
/// result is an int/uint.
fn integer_kind(args: &[V]) -> Option<Scalar> {
    match args.first() {
        Some(V::Vec {
            k: k @ (Scalar::Int | Scalar::UInt),
            ..
        }) => Some(*k),
        _ => None,
    }
}

fn map_int(args: &[V], f: impl Fn(&[i64]) -> i64) -> R<V> {
    let k = integer_kind(args).ok_or("expected integers")?;
    let n = args.iter().map(V::n).max().unwrap_or(1);
    let mut out = [0u32; 4];
    for i in 0..n {
        let lane: Vec<i64> = args
            .iter()
            .map(|a| match a {
                V::Vec { k, n, c } => {
                    let bits = c[if *n == 1 { 0 } else { i }];
                    if *k == Scalar::UInt {
                        i64::from(bits)
                    } else {
                        i64::from(bits as i32)
                    }
                }
                _ => 0,
            })
            .collect();
        out[i] = f(&lane) as u32;
    }
    Ok(V::Vec {
        k,
        n: n as u8,
        c: out,
    })
}

fn bool_vec(v: &V, other: &V, f: impl Fn(f32, f32) -> bool) -> R<V> {
    let ((n, a), (_, b)) = (floats(v)?, floats(other)?);
    let mut out = [0u32; 4];
    for i in 0..n as usize {
        out[i] = u32::from(f(a[i], b[i]));
    }
    Ok(V::Vec {
        k: Scalar::Bool,
        n,
        c: out,
    })
}

fn transpose(v: &V) -> R<V> {
    let V::Mat { cols, rows, d } = v else {
        return Err("transpose needs a matrix".into());
    };
    let mut out = [0.0; 16];
    for c in 0..*cols as usize {
        for r in 0..*rows as usize {
            out[r * *cols as usize + c] = d[c * *rows as usize + r];
        }
    }
    Ok(V::Mat {
        cols: *rows,
        rows: *cols,
        d: out,
    })
}

fn determinant(v: &V) -> R<f32> {
    let V::Mat { cols, d, .. } = v else {
        return Err("determinant needs a matrix".into());
    };
    let m = |c: usize, r: usize| d[c * *cols as usize + r];
    Ok(match cols {
        2 => m(0, 0) * m(1, 1) - m(1, 0) * m(0, 1),
        3 => {
            m(0, 0) * (m(1, 1) * m(2, 2) - m(2, 1) * m(1, 2))
                - m(1, 0) * (m(0, 1) * m(2, 2) - m(2, 1) * m(0, 2))
                + m(2, 0) * (m(0, 1) * m(1, 2) - m(1, 1) * m(0, 2))
        }
        4 => {
            // Laplace expansion along the first column.
            let mut det = 0.0;
            for r in 0..4 {
                let mut minor = [0.0f32; 9];
                let mut k = 0;
                for c in 1..4 {
                    for rr in 0..4 {
                        if rr != r {
                            minor[k] = m(c, rr);
                            k += 1;
                        }
                    }
                }
                let sub = V::Mat {
                    cols: 3,
                    rows: 3,
                    d: {
                        let mut a = [0.0; 16];
                        a[..9].copy_from_slice(&minor);
                        a
                    },
                };
                let cof = determinant(&sub)?;
                det += if r % 2 == 0 { 1.0 } else { -1.0 } * m(0, r) * cof;
            }
            det
        }
        _ => return Err("unsupported matrix size".into()),
    })
}

fn inverse(v: &V) -> R<V> {
    let V::Mat { cols, rows, d } = v else {
        return Err("inverse needs a matrix".into());
    };
    let n = *cols as usize;
    if *cols != *rows {
        return Err("inverse needs a square matrix".into());
    }
    // Gauss-Jordan on f64 for accuracy.
    let mut a = vec![vec![0.0f64; 2 * n]; n];
    for c in 0..n {
        for r in 0..n {
            a[r][c] = f64::from(d[c * n + r]);
        }
        a[c][n + c] = 1.0;
    }
    for col in 0..n {
        let pivot = (col..n)
            .max_by(|&x, &y| {
                a[x][col]
                    .abs()
                    .partial_cmp(&a[y][col].abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(col);
        if a[pivot][col].abs() < 1e-30 {
            return Ok(V::Mat {
                cols: *cols,
                rows: *rows,
                d: [f32::NAN; 16],
            });
        }
        a.swap(col, pivot);
        let p = a[col][col];
        for x in a[col].iter_mut() {
            *x /= p;
        }
        for r in 0..n {
            if r != col {
                let f = a[r][col];
                if f != 0.0 {
                    // Row `r` reads row `col` while being written: two rows of one array,
                    // so this is indexed, not iterated, and its order of operations is fixed.
                    #[allow(clippy::needless_range_loop)]
                    for c in 0..2 * n {
                        a[r][c] -= f * a[col][c];
                    }
                }
            }
        }
    }
    let mut out = [0.0f32; 16];
    for c in 0..n {
        for r in 0..n {
            out[c * n + r] = a[r][n + c] as f32;
        }
    }
    Ok(V::Mat {
        cols: *cols,
        rows: *rows,
        d: out,
    })
}

fn sampler(v: &V) -> R<(u32, SamplerKind)> {
    match v {
        V::Sampler(unit, kind) => Ok((*unit, *kind)),
        _ => Err("expected a sampler".into()),
    }
}

fn coord4(v: &V) -> R<[f32; 4]> {
    Ok(floats(v)?.1)
}

/// A built-in function, resolved from its name once, when a shader is lowered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Radians,
    Degrees,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    Asinh,
    Acosh,
    Atanh,
    Pow,
    Exp,
    Log,
    Exp2,
    Log2,
    Sqrt,
    Inversesqrt,
    Abs,
    Sign,
    Floor,
    Ceil,
    Trunc,
    Round,
    RoundEven,
    Fract,
    Mod,
    Min,
    Max,
    Clamp,
    Mix,
    Step,
    Smoothstep,
    Isnan,
    Isinf,
    Length,
    Distance,
    Dot,
    Cross,
    Normalize,
    Faceforward,
    Reflect,
    Refract,
    LessThan,
    LessThanEqual,
    GreaterThan,
    GreaterThanEqual,
    Equal,
    NotEqual,
    Any,
    All,
    Not,
    MatrixCompMult,
    OuterProduct,
    Transpose,
    Determinant,
    Inverse,
    DFdx,
    DFdy,
    Fwidth,
    FloatBitsToInt,
    FloatBitsToUint,
    IntBitsToFloat,
    UintBitsToFloat,
    Texture,
    Texture2D,
    TextureCube,
    Texture3D,
    Texture2DArray,
    TextureProj,
    Texture2DProj,
    TextureLod,
    Texture2DLod,
    TextureCubeLod,
    TexelFetch,
    TextureSize,
}

impl Builtin {
    pub fn from_name(name: &str) -> Option<Builtin> {
        Some(match name {
            "radians" => Builtin::Radians,
            "degrees" => Builtin::Degrees,
            "sin" => Builtin::Sin,
            "cos" => Builtin::Cos,
            "tan" => Builtin::Tan,
            "asin" => Builtin::Asin,
            "acos" => Builtin::Acos,
            "atan" => Builtin::Atan,
            "sinh" => Builtin::Sinh,
            "cosh" => Builtin::Cosh,
            "tanh" => Builtin::Tanh,
            "asinh" => Builtin::Asinh,
            "acosh" => Builtin::Acosh,
            "atanh" => Builtin::Atanh,
            "pow" => Builtin::Pow,
            "exp" => Builtin::Exp,
            "log" => Builtin::Log,
            "exp2" => Builtin::Exp2,
            "log2" => Builtin::Log2,
            "sqrt" => Builtin::Sqrt,
            "inversesqrt" => Builtin::Inversesqrt,
            "abs" => Builtin::Abs,
            "sign" => Builtin::Sign,
            "floor" => Builtin::Floor,
            "ceil" => Builtin::Ceil,
            "trunc" => Builtin::Trunc,
            "round" => Builtin::Round,
            "roundEven" => Builtin::RoundEven,
            "fract" => Builtin::Fract,
            "mod" => Builtin::Mod,
            "min" => Builtin::Min,
            "max" => Builtin::Max,
            "clamp" => Builtin::Clamp,
            "mix" => Builtin::Mix,
            "step" => Builtin::Step,
            "smoothstep" => Builtin::Smoothstep,
            "isnan" => Builtin::Isnan,
            "isinf" => Builtin::Isinf,
            "length" => Builtin::Length,
            "distance" => Builtin::Distance,
            "dot" => Builtin::Dot,
            "cross" => Builtin::Cross,
            "normalize" => Builtin::Normalize,
            "faceforward" => Builtin::Faceforward,
            "reflect" => Builtin::Reflect,
            "refract" => Builtin::Refract,
            "lessThan" => Builtin::LessThan,
            "lessThanEqual" => Builtin::LessThanEqual,
            "greaterThan" => Builtin::GreaterThan,
            "greaterThanEqual" => Builtin::GreaterThanEqual,
            "equal" => Builtin::Equal,
            "notEqual" => Builtin::NotEqual,
            "any" => Builtin::Any,
            "all" => Builtin::All,
            "not" => Builtin::Not,
            "matrixCompMult" => Builtin::MatrixCompMult,
            "outerProduct" => Builtin::OuterProduct,
            "transpose" => Builtin::Transpose,
            "determinant" => Builtin::Determinant,
            "inverse" => Builtin::Inverse,
            "dFdx" => Builtin::DFdx,
            "dFdy" => Builtin::DFdy,
            "fwidth" => Builtin::Fwidth,
            "floatBitsToInt" => Builtin::FloatBitsToInt,
            "floatBitsToUint" => Builtin::FloatBitsToUint,
            "intBitsToFloat" => Builtin::IntBitsToFloat,
            "uintBitsToFloat" => Builtin::UintBitsToFloat,
            "texture" => Builtin::Texture,
            "texture2D" => Builtin::Texture2D,
            "textureCube" => Builtin::TextureCube,
            "texture3D" => Builtin::Texture3D,
            "texture2DArray" => Builtin::Texture2DArray,
            "textureProj" => Builtin::TextureProj,
            "texture2DProj" => Builtin::Texture2DProj,
            "textureLod" => Builtin::TextureLod,
            "texture2DLod" => Builtin::Texture2DLod,
            "textureCubeLod" => Builtin::TextureCubeLod,
            "texelFetch" => Builtin::TexelFetch,
            "textureSize" => Builtin::TextureSize,
            _ => return None,
        })
    }
}

/// Calls built-in `name`, or `None` when it is not one.
pub fn call(name: &str, args: &[V], textures: &dyn Textures) -> Option<R<V>> {
    call_resolved(Builtin::from_name(name)?, name, args, textures)
}

/// Calls the already-resolved built-in `which`, named `name` (for messages and the few
/// variants that read their own name).
pub fn call_resolved(
    which: Builtin,
    name: &str,
    args: &[V],
    textures: &dyn Textures,
) -> Option<R<V>> {
    let a = |i: usize| {
        args.get(i)
            .ok_or_else(|| format!("{name}: missing argument {i}"))
    };
    let r: R<V> = (|| match which {
        Builtin::Radians => map1(a(0)?, f32::to_radians),
        Builtin::Degrees => map1(a(0)?, f32::to_degrees),
        Builtin::Sin => map1(a(0)?, f32::sin),
        Builtin::Cos => map1(a(0)?, f32::cos),
        Builtin::Tan => map1(a(0)?, f32::tan),
        Builtin::Asin => map1(a(0)?, f32::asin),
        Builtin::Acos => map1(a(0)?, f32::acos),
        Builtin::Atan if args.len() == 1 => map1(a(0)?, f32::atan),
        Builtin::Atan => map2(a(0)?, a(1)?, f32::atan2),
        Builtin::Sinh => map1(a(0)?, f32::sinh),
        Builtin::Cosh => map1(a(0)?, f32::cosh),
        Builtin::Tanh => map1(a(0)?, f32::tanh),
        Builtin::Asinh => map1(a(0)?, f32::asinh),
        Builtin::Acosh => map1(a(0)?, f32::acosh),
        Builtin::Atanh => map1(a(0)?, f32::atanh),
        Builtin::Pow => map2(a(0)?, a(1)?, f32::powf),
        Builtin::Exp => map1(a(0)?, f32::exp),
        Builtin::Log => map1(a(0)?, f32::ln),
        Builtin::Exp2 => map1(a(0)?, f32::exp2),
        Builtin::Log2 => map1(a(0)?, f32::log2),
        Builtin::Sqrt => map1(a(0)?, f32::sqrt),
        Builtin::Inversesqrt => map1(a(0)?, |x| 1.0 / x.sqrt()),
        Builtin::Abs if integer_kind(args).is_some() => map_int(args, |l| l[0].abs()),
        Builtin::Abs => map1(a(0)?, f32::abs),
        Builtin::Sign if integer_kind(args).is_some() => map_int(args, |l| l[0].signum()),
        Builtin::Sign => map1(a(0)?, |x| {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }),
        Builtin::Floor => map1(a(0)?, f32::floor),
        Builtin::Ceil => map1(a(0)?, f32::ceil),
        Builtin::Trunc => map1(a(0)?, f32::trunc),
        Builtin::Round => map1(a(0)?, f32::round),
        Builtin::RoundEven => map1(a(0)?, round_even),
        Builtin::Fract => map1(a(0)?, |x| x - x.floor()),
        Builtin::Mod => map2(a(0)?, a(1)?, mod_f),
        Builtin::Min if integer_kind(args).is_some() => map_int(args, |l| l[0].min(l[1])),
        Builtin::Max if integer_kind(args).is_some() => map_int(args, |l| l[0].max(l[1])),
        Builtin::Min => map2(a(0)?, a(1)?, f32::min),
        Builtin::Max => map2(a(0)?, a(1)?, f32::max),
        Builtin::Clamp if integer_kind(args).is_some() => {
            map_int(args, |l| l[0].clamp(l[1].min(l[2]), l[2]))
        }
        Builtin::Clamp => map3(a(0)?, a(1)?, a(2)?, |x, lo, hi| x.max(lo).min(hi)),
        Builtin::Mix => {
            if matches!(
                args.get(2),
                Some(V::Vec {
                    k: Scalar::Bool,
                    ..
                })
            ) {
                // mix(x, y, bvec): select.
                let (V::Vec { n, c: cx, k: kx }, V::Vec { c: cy, .. }, V::Vec { c: cs, n: ns, .. }) =
                    (a(0)?, a(1)?, a(2)?)
                else {
                    return Err("bad mix".into());
                };
                let mut out = [0u32; 4];
                for i in 0..*n as usize {
                    out[i] = if cs[if *ns == 1 { 0 } else { i }] != 0 {
                        cy[i]
                    } else {
                        cx[i]
                    };
                }
                Ok(V::Vec {
                    k: *kx,
                    n: *n,
                    c: out,
                })
            } else {
                map3(a(0)?, a(1)?, a(2)?, |x, y, t| x * (1.0 - t) + y * t)
            }
        }
        Builtin::Step => map2(a(0)?, a(1)?, |edge, x| if x < edge { 0.0 } else { 1.0 }),
        Builtin::Smoothstep => map3(a(0)?, a(1)?, a(2)?, |e0, e1, x| {
            let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }),
        Builtin::Isnan => Ok(bool_of(a(0)?, f32::is_nan)?),
        Builtin::Isinf => Ok(bool_of(a(0)?, f32::is_infinite)?),
        Builtin::Length => {
            let v = a(0)?;
            Ok(V::f(dot(v, v)?.sqrt()))
        }
        Builtin::Distance => {
            let d = binary(BinOp::Sub, a(0)?, a(1)?)?;
            Ok(V::f(dot(&d, &d)?.sqrt()))
        }
        Builtin::Dot => Ok(V::f(dot(a(0)?, a(1)?)?)),
        Builtin::Cross => {
            let ((_, x), (_, y)) = (floats(a(0)?)?, floats(a(1)?)?);
            Ok(from_floats(
                3,
                [
                    x[1] * y[2] - x[2] * y[1],
                    x[2] * y[0] - x[0] * y[2],
                    x[0] * y[1] - x[1] * y[0],
                    0.0,
                ],
            ))
        }
        Builtin::Normalize => {
            let v = a(0)?;
            let len = dot(v, v)?.sqrt();
            map1(v, |x| x / len)
        }
        Builtin::Faceforward => {
            let d = dot(a(2)?, a(1)?)?;
            if d < 0.0 {
                Ok(a(0)?.clone())
            } else {
                negate(a(0)?)
            }
        }
        Builtin::Reflect => {
            let d = dot(a(1)?, a(0)?)?;
            let scaled = binary(BinOp::Mul, a(1)?, &V::f(2.0 * d))?;
            binary(BinOp::Sub, a(0)?, &scaled)
        }
        Builtin::Refract => {
            let eta = a(2)?.as_f32().ok_or("refract needs a float eta")?;
            let d = dot(a(1)?, a(0)?)?;
            let k = 1.0 - eta * eta * (1.0 - d * d);
            if k < 0.0 {
                map1(a(0)?, |_| 0.0)
            } else {
                let i = binary(BinOp::Mul, a(0)?, &V::f(eta))?;
                let n = binary(BinOp::Mul, a(1)?, &V::f(eta * d + k.sqrt()))?;
                binary(BinOp::Sub, &i, &n)
            }
        }
        Builtin::LessThan => bool_vec(a(0)?, a(1)?, |x, y| x < y),
        Builtin::LessThanEqual => bool_vec(a(0)?, a(1)?, |x, y| x <= y),
        Builtin::GreaterThan => bool_vec(a(0)?, a(1)?, |x, y| x > y),
        Builtin::GreaterThanEqual => bool_vec(a(0)?, a(1)?, |x, y| x >= y),
        Builtin::Equal => bool_vec(a(0)?, a(1)?, |x, y| x == y),
        Builtin::NotEqual => bool_vec(a(0)?, a(1)?, |x, y| x != y),
        Builtin::Any => {
            let (n, c) = floats(a(0)?)?;
            Ok(V::b((0..n as usize).any(|i| c[i] != 0.0)))
        }
        Builtin::All => {
            let (n, c) = floats(a(0)?)?;
            Ok(V::b((0..n as usize).all(|i| c[i] != 0.0)))
        }
        Builtin::Not => logical_not(a(0)?),
        Builtin::MatrixCompMult => {
            let (V::Mat { cols, rows, d: d1 }, V::Mat { d: d2, .. }) = (a(0)?, a(1)?) else {
                return Err("matrixCompMult needs matrices".into());
            };
            let mut d = [0.0; 16];
            for i in 0..16 {
                d[i] = d1[i] * d2[i];
            }
            Ok(V::Mat {
                cols: *cols,
                rows: *rows,
                d,
            })
        }
        Builtin::OuterProduct => {
            let ((nr, c), (nc, r)) = (floats(a(0)?)?, floats(a(1)?)?);
            let mut d = [0.0; 16];
            for col in 0..nc as usize {
                for row in 0..nr as usize {
                    d[col * nr as usize + row] = c[row] * r[col];
                }
            }
            Ok(V::Mat {
                cols: nc,
                rows: nr,
                d,
            })
        }
        Builtin::Transpose => transpose(a(0)?),
        Builtin::Determinant => Ok(V::f(determinant(a(0)?)?)),
        Builtin::Inverse => inverse(a(0)?),
        Builtin::DFdx | Builtin::DFdy | Builtin::Fwidth => map1(a(0)?, |_| 0.0),
        Builtin::FloatBitsToInt => map_bits(a(0)?, Scalar::Int),
        Builtin::FloatBitsToUint => map_bits(a(0)?, Scalar::UInt),
        Builtin::IntBitsToFloat | Builtin::UintBitsToFloat => map_bits(a(0)?, Scalar::Float),
        Builtin::Texture
        | Builtin::Texture2D
        | Builtin::TextureCube
        | Builtin::Texture3D
        | Builtin::Texture2DArray
        | Builtin::TextureProj
        | Builtin::Texture2DProj => {
            let (unit, kind) = sampler(a(0)?)?;
            let mut coord = coord4(a(1)?)?;
            let proj = name.contains("Proj");
            if proj {
                let n = a(1)?.n();
                let w = coord[n - 1];
                for c in coord.iter_mut().take(n - 1) {
                    *c /= w;
                }
            }
            let bias = args.get(2).and_then(V::as_f32).unwrap_or(0.0);
            Ok(from_floats(
                4,
                textures.sample(unit, kind, coord, None, bias),
            ))
        }
        Builtin::TextureLod | Builtin::Texture2DLod | Builtin::TextureCubeLod => {
            let (unit, kind) = sampler(a(0)?)?;
            let coord = coord4(a(1)?)?;
            let lod = a(2)?.as_f32().unwrap_or(0.0);
            Ok(from_floats(
                4,
                textures.sample(unit, kind, coord, Some(lod), 0.0),
            ))
        }
        Builtin::TexelFetch => {
            let (unit, kind) = sampler(a(0)?)?;
            let (_, c) = floats(a(1)?)?;
            let lod = a(2)?.as_i32().unwrap_or(0);
            Ok(from_floats(
                4,
                textures.fetch(unit, kind, [c[0] as i32, c[1] as i32, c[2] as i32], lod),
            ))
        }
        Builtin::TextureSize => {
            let (unit, kind) = sampler(a(0)?)?;
            let s = textures.size(unit, kind, a(1)?.as_i32().unwrap_or(0));
            let lanes = if matches!(kind, SamplerKind::S3D | SamplerKind::S2DArray) {
                3
            } else {
                2
            };
            Ok(V::of_kind(Scalar::Int, &s.map(|x| x as u32)[..lanes]))
        }
    })();
    Some(r)
}

fn bool_of(v: &V, f: impl Fn(f32) -> bool) -> R<V> {
    let (n, c) = floats(v)?;
    let mut out = [0u32; 4];
    for i in 0..n as usize {
        out[i] = u32::from(f(c[i]));
    }
    Ok(V::Vec {
        k: Scalar::Bool,
        n,
        c: out,
    })
}

fn map_bits(v: &V, to: Scalar) -> R<V> {
    match v {
        V::Vec { n, c, .. } => Ok(V::Vec {
            k: to,
            n: *n,
            c: *c,
        }),
        _ => Err("expected a number".into()),
    }
}

pub fn is_builtin(name: &str) -> bool {
    Builtin::from_name(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str, args: &[V]) -> V {
        call(name, args, &NoTextures)
            .expect("a built-in")
            .expect("no error")
    }
    fn f(v: V) -> Vec<f32> {
        v.floats().unwrap()
    }
    fn close(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-5, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn scalar_math_maps_over_vectors() {
        close(&f(run("sqrt", &[V::vec(&[4.0, 9.0])])), &[2.0, 3.0]);
        close(&f(run("fract", &[V::f(-0.25)])), &[0.75]);
        close(&f(run("mod", &[V::f(-1.0), V::f(3.0)])), &[2.0]);
        close(
            &f(run("pow", &[V::vec(&[2.0, 3.0]), V::f(2.0)])),
            &[4.0, 9.0],
        );
        close(
            &f(run("atan", &[V::f(1.0), V::f(1.0)])),
            &[std::f32::consts::FRAC_PI_4],
        );
        close(
            &f(run("roundEven", &[V::vec(&[0.5, 1.5, 2.5])])),
            &[0.0, 2.0, 2.0],
        );
        close(
            &f(run("sign", &[V::vec(&[-3.0, 0.0, 2.0])])),
            &[-1.0, 0.0, 1.0],
        );
    }

    #[test]
    fn interpolation_and_clamping() {
        close(&f(run("mix", &[V::f(0.0), V::f(10.0), V::f(0.25)])), &[2.5]);
        close(
            &f(run(
                "clamp",
                &[V::vec(&[-1.0, 0.5, 9.0]), V::f(0.0), V::f(1.0)],
            )),
            &[0.0, 0.5, 1.0],
        );
        close(
            &f(run("smoothstep", &[V::f(0.0), V::f(1.0), V::f(0.5)])),
            &[0.5],
        );
        close(
            &f(run("step", &[V::f(0.5), V::vec(&[0.4, 0.5])])),
            &[0.0, 1.0],
        );
        assert_eq!(run("min", &[V::i(3), V::i(-2)]), V::i(-2));
        assert_eq!(run("clamp", &[V::i(9), V::i(0), V::i(5)]), V::i(5));
        assert_eq!(run("abs", &[V::i(-4)]), V::i(4));
        let picked = run(
            "mix",
            &[
                V::vec(&[1.0, 2.0]),
                V::vec(&[3.0, 4.0]),
                V::of_kind(Scalar::Bool, &[1, 0]),
            ],
        );
        assert_eq!(f(picked), vec![3.0, 2.0]);
    }

    #[test]
    fn geometry_functions() {
        close(&f(run("length", &[V::vec(&[3.0, 4.0])])), &[5.0]);
        close(
            &f(run("normalize", &[V::vec(&[0.0, 3.0, 4.0])])),
            &[0.0, 0.6, 0.8],
        );
        close(
            &f(run(
                "dot",
                &[V::vec(&[1.0, 2.0, 3.0]), V::vec(&[4.0, 5.0, 6.0])],
            )),
            &[32.0],
        );
        close(
            &f(run(
                "cross",
                &[V::vec(&[1.0, 0.0, 0.0]), V::vec(&[0.0, 1.0, 0.0])],
            )),
            &[0.0, 0.0, 1.0],
        );
        close(
            &f(run("distance", &[V::vec(&[1.0, 1.0]), V::vec(&[4.0, 5.0])])),
            &[5.0],
        );
        close(
            &f(run("reflect", &[V::vec(&[1.0, -1.0]), V::vec(&[0.0, 1.0])])),
            &[1.0, 1.0],
        );
        close(
            &f(run(
                "refract",
                &[V::vec(&[0.0, -1.0]), V::vec(&[0.0, 1.0]), V::f(1.0)],
            )),
            &[0.0, -1.0],
        );
    }

    #[test]
    fn matrix_functions() {
        let m = V::Mat {
            cols: 2,
            rows: 2,
            d: {
                let mut d = [0.0; 16];
                d[..4].copy_from_slice(&[4.0, 2.0, 7.0, 6.0]);
                d
            },
        };
        close(&f(run("determinant", std::slice::from_ref(&m))), &[10.0]);
        let inv = run("inverse", std::slice::from_ref(&m));
        let product = binary(BinOp::Mul, &m, &inv).unwrap();
        let V::Mat { d, .. } = product else { panic!() };
        close(&d[..4], &[1.0, 0.0, 0.0, 1.0]);
        let t = run("transpose", &[m]);
        let V::Mat { d, .. } = t else { panic!() };
        close(&d[..4], &[4.0, 7.0, 2.0, 6.0]);
        // A 4x4 (translate) inverse and determinant.
        let mut d = [0.0; 16];
        for i in 0..4 {
            d[i * 4 + i] = 2.0;
        }
        d[12] = 5.0;
        let m4 = V::Mat {
            cols: 4,
            rows: 4,
            d,
        };
        close(&f(run("determinant", std::slice::from_ref(&m4))), &[16.0]);
        let inv = run("inverse", std::slice::from_ref(&m4));
        let V::Mat { d, .. } = binary(BinOp::Mul, &m4, &inv).unwrap() else {
            panic!()
        };
        for i in 0..4 {
            for j in 0..4 {
                assert!((d[i * 4 + j] - if i == j { 1.0 } else { 0.0 }).abs() < 1e-5);
            }
        }
        let op = run("outerProduct", &[V::vec(&[1.0, 2.0]), V::vec(&[3.0, 4.0])]);
        let V::Mat { d, .. } = op else { panic!() };
        close(&d[..4], &[3.0, 6.0, 4.0, 8.0]);
    }

    #[test]
    fn comparisons_and_reductions() {
        assert_eq!(
            run("lessThan", &[V::vec(&[1.0, 5.0]), V::vec(&[2.0, 2.0])]),
            V::of_kind(Scalar::Bool, &[1, 0])
        );
        assert_eq!(run("any", &[V::of_kind(Scalar::Bool, &[0, 1])]), V::b(true));
        assert_eq!(
            run("all", &[V::of_kind(Scalar::Bool, &[0, 1])]),
            V::b(false)
        );
        assert_eq!(run("isnan", &[V::f(f32::NAN)]), V::b(true));
    }

    #[test]
    fn a_name_resolves_to_the_built_in_that_call_by_name_runs() {
        let one = V::f(0.5);
        for name in ["sin", "abs", "exp2", "sqrt", "radians", "fract", "sign"] {
            let which = Builtin::from_name(name).expect(name);
            let by_id = call_resolved(which, name, &[one.clone()], &NoTextures);
            assert_eq!(by_id, call(name, &[one.clone()], &NoTextures), "{name}");
        }
        assert_eq!(Builtin::from_name("myFunction"), None);
        assert!(call("myFunction", &[], &NoTextures).is_none());
    }

    #[test]
    fn bit_reinterpretation_and_unknown_names() {
        assert_eq!(
            run("floatBitsToInt", &[V::f(1.0)]).as_i32(),
            Some(0x3f800000)
        );
        assert_eq!(
            run("intBitsToFloat", &[V::i(0x3f800000)]).as_f32(),
            Some(1.0)
        );
        assert!(call("noSuchThing", &[], &NoTextures).is_none());
        assert!(is_builtin("texture2D") && !is_builtin("myFunction"));
    }

    struct Gradient;
    impl Textures for Gradient {
        fn sample(
            &self,
            unit: u32,
            _: SamplerKind,
            c: [f32; 4],
            lod: Option<f32>,
            bias: f32,
        ) -> [f32; 4] {
            [c[0], c[1], unit as f32 + lod.unwrap_or(0.0) + bias, 1.0]
        }
        fn fetch(&self, _: u32, _: SamplerKind, c: [i32; 3], lod: i32) -> [f32; 4] {
            [c[0] as f32, c[1] as f32, lod as f32, 1.0]
        }
        fn size(&self, _: u32, _: SamplerKind, lod: i32) -> [i32; 3] {
            [8 >> lod, 4 >> lod, 1]
        }
    }

    #[test]
    fn texture_functions_go_through_the_sampler_source() {
        let s = V::Sampler(3, SamplerKind::S2D);
        let r = call("texture2D", &[s.clone(), V::vec(&[0.25, 0.5])], &Gradient)
            .unwrap()
            .unwrap();
        assert_eq!(f(r), vec![0.25, 0.5, 3.0, 1.0]);
        let r = call(
            "textureLod",
            &[s.clone(), V::vec(&[0.0, 0.0]), V::f(2.0)],
            &Gradient,
        )
        .unwrap()
        .unwrap();
        assert_eq!(f(r)[2], 5.0);
        let r = call(
            "texelFetch",
            &[s.clone(), V::of_kind(Scalar::Int, &[2, 1]), V::i(1)],
            &Gradient,
        )
        .unwrap()
        .unwrap();
        assert_eq!(f(r), vec![2.0, 1.0, 1.0, 1.0]);
        let r = call("textureSize", &[s, V::i(1)], &Gradient)
            .unwrap()
            .unwrap();
        assert_eq!(r, V::of_kind(Scalar::Int, &[4, 2]));
        let r = call(
            "textureProj",
            &[V::Sampler(0, SamplerKind::S2D), V::vec(&[2.0, 4.0, 2.0])],
            &Gradient,
        )
        .unwrap()
        .unwrap();
        assert_eq!(f(r)[..2], [1.0, 2.0]);
    }
}
