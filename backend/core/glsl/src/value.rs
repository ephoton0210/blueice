// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Run-time values of the shader interpreter and the arithmetic on them.
//!
//! Values carry their own scalar kind, so the interpreter needs no static type
//! checker: shaders are assumed valid, and a declaration, assignment, argument
//! or return converts to the declared type where it matters.

use super::ast::*;

#[derive(Debug, Clone, PartialEq)]
pub enum V {
    /// A scalar (`n == 1`) or a vector of 2 to 4 components; lanes are raw bits.
    Vec {
        k: Scalar,
        n: u8,
        c: [u32; 4],
    },
    /// A matrix, column-major, `cols` columns of `rows` rows.
    Mat {
        cols: u8,
        rows: u8,
        d: [f32; 16],
    },
    Struct(usize, Vec<V>),
    Arr(Vec<V>),
    /// A sampler: the texture unit it reads.
    Sampler(u32, SamplerKind),
    Void,
}

pub type R<T> = Result<T, String>;

impl V {
    pub fn f(x: f32) -> V {
        V::Vec {
            k: Scalar::Float,
            n: 1,
            c: [x.to_bits(), 0, 0, 0],
        }
    }
    pub fn i(x: i32) -> V {
        V::Vec {
            k: Scalar::Int,
            n: 1,
            c: [x as u32, 0, 0, 0],
        }
    }
    pub fn u(x: u32) -> V {
        V::Vec {
            k: Scalar::UInt,
            n: 1,
            c: [x, 0, 0, 0],
        }
    }
    pub fn b(x: bool) -> V {
        V::Vec {
            k: Scalar::Bool,
            n: 1,
            c: [u32::from(x), 0, 0, 0],
        }
    }
    pub fn vec(xs: &[f32]) -> V {
        let mut c = [0u32; 4];
        for (slot, x) in c.iter_mut().zip(xs) {
            *slot = x.to_bits();
        }
        V::Vec {
            k: Scalar::Float,
            n: xs.len() as u8,
            c,
        }
    }
    pub fn vec4(x: f32, y: f32, z: f32, w: f32) -> V {
        V::vec(&[x, y, z, w])
    }
    pub fn of_kind(k: Scalar, lanes: &[u32]) -> V {
        let mut c = [0u32; 4];
        for (slot, x) in c.iter_mut().zip(lanes) {
            *slot = *x;
        }
        V::Vec {
            k,
            n: lanes.len() as u8,
            c,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            V::Vec { k, c, .. } => Some(lane_f32(*k, c[0])),
            _ => None,
        }
    }
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            V::Vec { k, c, .. } => Some(match k {
                Scalar::Float => f32::from_bits(c[0]) as i32,
                _ => c[0] as i32,
            }),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            V::Vec { k, c, .. } => Some(match k {
                Scalar::Float => f32::from_bits(c[0]) != 0.0,
                _ => c[0] != 0,
            }),
            _ => None,
        }
    }
    /// The float lanes of a float vector or scalar.
    pub fn floats(&self) -> Option<Vec<f32>> {
        match self {
            V::Vec { k, n, c } => Some((0..*n as usize).map(|i| lane_f32(*k, c[i])).collect()),
            _ => None,
        }
    }
    pub fn n(&self) -> usize {
        match self {
            V::Vec { n, .. } => *n as usize,
            _ => 1,
        }
    }
}

pub fn lane_f32(k: Scalar, bits: u32) -> f32 {
    match k {
        Scalar::Float => f32::from_bits(bits),
        Scalar::Int => bits as i32 as f32,
        Scalar::UInt => bits as f32,
        Scalar::Bool => f32::from(bits != 0),
    }
}

fn lane_i32(k: Scalar, bits: u32) -> i32 {
    match k {
        Scalar::Float => f32::from_bits(bits) as i32,
        _ => bits as i32,
    }
}

fn lane_u32(k: Scalar, bits: u32) -> u32 {
    match k {
        Scalar::Float => f32::from_bits(bits) as u32,
        _ => bits,
    }
}

fn lane_bool(k: Scalar, bits: u32) -> bool {
    match k {
        Scalar::Float => f32::from_bits(bits) != 0.0,
        _ => bits != 0,
    }
}

/// One lane converted to scalar kind `to`.
pub fn convert_lane(from: Scalar, bits: u32, to: Scalar) -> u32 {
    match to {
        Scalar::Float => lane_f32(from, bits).to_bits(),
        Scalar::Int => lane_i32(from, bits) as u32,
        Scalar::UInt => lane_u32(from, bits),
        Scalar::Bool => u32::from(lane_bool(from, bits)),
    }
}

/// The zero value of a type.
pub fn zero(ty: &Ty, structs: &[StructDef]) -> V {
    match ty {
        Ty::Void => V::Void,
        Ty::Scalar(k) => V::Vec {
            k: *k,
            n: 1,
            c: [0; 4],
        },
        Ty::Vec(k, n) => V::Vec {
            k: *k,
            n: *n,
            c: [0; 4],
        },
        Ty::Mat(c, r) => V::Mat {
            cols: *c,
            rows: *r,
            d: [0.0; 16],
        },
        Ty::Sampler(kind) => V::Sampler(0, *kind),
        Ty::Struct(name) => {
            let (index, def) = structs
                .iter()
                .enumerate()
                .find(|(_, s)| &s.name == name)
                .expect("struct types are checked when a shader is compiled");
            V::Struct(
                index,
                def.fields.iter().map(|(_, t)| zero(t, structs)).collect(),
            )
        }
        Ty::Array(inner, n) => V::Arr((0..*n).map(|_| zero(inner, structs)).collect()),
    }
}

/// Converts `v` to the shape and scalar kind of `ty` where that is a plain
/// conversion (an `int` given to a `float`, say); anything else is returned
/// unchanged.
pub fn coerce(v: V, ty: &Ty) -> V {
    match (v, ty) {
        (V::Vec { k, n, c }, Ty::Scalar(to)) if n >= 1 && k != *to => V::Vec {
            k: *to,
            n: 1,
            c: [convert_lane(k, c[0], *to), 0, 0, 0],
        },
        (V::Vec { k, n, c }, Ty::Vec(to, m)) if k != *to && n == *m => {
            let mut out = [0u32; 4];
            for i in 0..n as usize {
                out[i] = convert_lane(k, c[i], *to);
            }
            V::Vec { k: *to, n, c: out }
        }
        (V::Vec { k, n: 1, c }, Ty::Vec(to, m)) => {
            // A scalar splats into a vector only through a constructor, but an
            // old-style `vec3 v = 1.0;` is tolerated.
            let lane = convert_lane(k, c[0], *to);
            V::Vec {
                k: *to,
                n: *m,
                c: [lane; 4],
            }
        }
        (V::Arr(items), Ty::Array(inner, _)) => {
            V::Arr(items.into_iter().map(|i| coerce(i, inner)).collect())
        }
        (v, _) => v,
    }
}

fn float_op(op: BinOp, a: f32, b: f32) -> R<f32> {
    Ok(match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        _ => return Err(format!("operator {op:?} does not apply to floats")),
    })
}

fn int_op(op: BinOp, a: i32, b: i32) -> R<i32> {
    Ok(match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::Div => {
            if b == 0 {
                0
            } else {
                a.wrapping_div(b)
            }
        }
        BinOp::Rem => {
            if b == 0 {
                0
            } else {
                a.wrapping_rem(b)
            }
        }
        BinOp::Shl => a.wrapping_shl(b as u32 & 31),
        BinOp::Shr => a.wrapping_shr(b as u32 & 31),
        BinOp::BitAnd => a & b,
        BinOp::BitOr => a | b,
        BinOp::BitXor => a ^ b,
        _ => return Err(format!("operator {op:?} does not apply to ints")),
    })
}

fn uint_op(op: BinOp, a: u32, b: u32) -> R<u32> {
    Ok(match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::Div => a.checked_div(b).unwrap_or(0),
        BinOp::Rem => a.checked_rem(b).unwrap_or(0),
        BinOp::Shl => a.wrapping_shl(b & 31),
        BinOp::Shr => a.wrapping_shr(b & 31),
        BinOp::BitAnd => a & b,
        BinOp::BitOr => a | b,
        BinOp::BitXor => a ^ b,
        _ => return Err(format!("operator {op:?} does not apply to uints")),
    })
}

/// Deep equality as `==` means it for any two values.
pub fn equal(a: &V, b: &V) -> bool {
    match (a, b) {
        (
            V::Vec {
                k: ka,
                n: na,
                c: ca,
            },
            V::Vec {
                k: kb,
                n: nb,
                c: cb,
            },
        ) => {
            na == nb
                && (0..*na as usize).all(|i| {
                    if *ka == Scalar::Float || *kb == Scalar::Float {
                        lane_f32(*ka, ca[i]) == lane_f32(*kb, cb[i])
                    } else {
                        ca[i] == cb[i]
                    }
                })
        }
        (
            V::Mat {
                cols: c1,
                rows: r1,
                d: d1,
            },
            V::Mat {
                cols: c2,
                rows: r2,
                d: d2,
            },
        ) => c1 == c2 && r1 == r2 && d1[..(*c1 * *r1) as usize] == d2[..(*c2 * *r2) as usize],
        (V::Struct(i, a), V::Struct(j, b)) => i == j && a.iter().zip(b).all(|(x, y)| equal(x, y)),
        (V::Arr(a), V::Arr(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| equal(x, y)),
        _ => false,
    }
}

/// The kind two operands compute in: float wins, then uint, then int.
fn common_kind(a: Scalar, b: Scalar) -> Scalar {
    use Scalar::*;
    match (a, b) {
        (Float, _) | (_, Float) => Float,
        (UInt, _) | (_, UInt) => UInt,
        (Int, _) | (_, Int) => Int,
        _ => Bool,
    }
}

pub fn binary(op: BinOp, a: &V, b: &V) -> R<V> {
    use BinOp::*;
    // The common case, float arithmetic on equal shapes or a scalar and a vector, with no
    // kind conversion to do.
    if let (
        V::Vec {
            k: Scalar::Float,
            n: na,
            c: ca,
        },
        V::Vec {
            k: Scalar::Float,
            n: nb,
            c: cb,
        },
    ) = (a, b)
    {
        if (*na == *nb || *na == 1 || *nb == 1) && matches!(op, Add | Sub | Mul | Div) {
            let n = (*na).max(*nb);
            let mut out = [0u32; 4];
            for i in 0..n as usize {
                let x = f32::from_bits(ca[if *na == 1 { 0 } else { i }]);
                let y = f32::from_bits(cb[if *nb == 1 { 0 } else { i }]);
                out[i] = match op {
                    Add => x + y,
                    Sub => x - y,
                    Mul => x * y,
                    _ => x / y,
                }
                .to_bits();
            }
            return Ok(V::Vec {
                k: Scalar::Float,
                n,
                c: out,
            });
        }
    }
    match op {
        Eq => return Ok(V::b(equal(a, b))),
        Ne => return Ok(V::b(!equal(a, b))),
        And | Or | Xor => {
            let (x, y) = (
                a.as_bool().ok_or("a logical operator needs bools")?,
                b.as_bool().ok_or("a logical operator needs bools")?,
            );
            return Ok(V::b(match op {
                And => x && y,
                Or => x || y,
                _ => x ^ y,
            }));
        }
        Lt | Gt | Le | Ge => {
            let (V::Vec { k: ka, c: ca, .. }, V::Vec { k: kb, c: cb, .. }) = (a, b) else {
                return Err("a comparison needs scalars".into());
            };
            let k = common_kind(*ka, *kb);
            let (x, y) = (convert_lane(*ka, ca[0], k), convert_lane(*kb, cb[0], k));
            let ord = match k {
                Scalar::Float => f32::from_bits(x).partial_cmp(&f32::from_bits(y)),
                Scalar::Int => Some((x as i32).cmp(&(y as i32))),
                _ => Some(x.cmp(&y)),
            };
            return Ok(V::b(match (op, ord) {
                (_, None) => false,
                (Lt, Some(o)) => o.is_lt(),
                (Gt, Some(o)) => o.is_gt(),
                (Le, Some(o)) => o.is_le(),
                (_, Some(o)) => o.is_ge(),
            }));
        }
        _ => {}
    }
    match (a, b) {
        (
            V::Vec {
                k: ka,
                n: na,
                c: ca,
            },
            V::Vec {
                k: kb,
                n: nb,
                c: cb,
            },
        ) => {
            let n = (*na).max(*nb);
            if *na != *nb && *na != 1 && *nb != 1 {
                return Err("vector sizes do not match".into());
            }
            let k = common_kind(*ka, *kb);
            let mut out = [0u32; 4];
            for i in 0..n as usize {
                let x = convert_lane(*ka, ca[if *na == 1 { 0 } else { i }], k);
                let y = convert_lane(*kb, cb[if *nb == 1 { 0 } else { i }], k);
                out[i] = match k {
                    Scalar::Float => float_op(op, f32::from_bits(x), f32::from_bits(y))?.to_bits(),
                    Scalar::Int => int_op(op, x as i32, y as i32)? as u32,
                    Scalar::UInt => uint_op(op, x, y)?,
                    Scalar::Bool => return Err("arithmetic on bools".into()),
                };
            }
            Ok(V::Vec { k, n, c: out })
        }
        (
            V::Mat {
                cols: c1,
                rows: r1,
                d: d1,
            },
            V::Mat {
                cols: c2,
                rows: r2,
                d: d2,
            },
        ) => {
            if op == Mul {
                if c1 != r2 {
                    return Err("matrix sizes do not multiply".into());
                }
                let (n, m, p) = (*r1 as usize, *c1 as usize, *c2 as usize);
                let mut d = [0.0f32; 16];
                for col in 0..p {
                    for row in 0..n {
                        let mut s = 0.0;
                        for k in 0..m {
                            s += d1[k * n + row] * d2[col * m + k];
                        }
                        d[col * n + row] = s;
                    }
                }
                Ok(V::Mat {
                    cols: *c2,
                    rows: *r1,
                    d,
                })
            } else {
                if (c1, r1) != (c2, r2) {
                    return Err("matrix sizes do not match".into());
                }
                let mut d = [0.0f32; 16];
                for i in 0..(*c1 * *r1) as usize {
                    d[i] = float_op(op, d1[i], d2[i])?;
                }
                Ok(V::Mat {
                    cols: *c1,
                    rows: *r1,
                    d,
                })
            }
        }
        (V::Mat { cols, rows, d }, V::Vec { n, k, c }) => {
            // mat * vec
            if op != Mul || *cols as usize != *n as usize {
                if *n == 1 {
                    let s = lane_f32(*k, c[0]);
                    let mut out = [0.0f32; 16];
                    for i in 0..(*cols * *rows) as usize {
                        out[i] = float_op(op, d[i], s)?;
                    }
                    return Ok(V::Mat {
                        cols: *cols,
                        rows: *rows,
                        d: out,
                    });
                }
                return Err("matrix and vector sizes do not match".into());
            }
            let mut out = [0u32; 4];
            for row in 0..*rows as usize {
                let mut s = 0.0;
                for col in 0..*cols as usize {
                    s += d[col * *rows as usize + row] * lane_f32(*k, c[col]);
                }
                out[row] = s.to_bits();
            }
            Ok(V::Vec {
                k: Scalar::Float,
                n: *rows,
                c: out,
            })
        }
        (V::Vec { n, k, c }, V::Mat { cols, rows, d }) => {
            if op == Mul && *rows as usize == *n as usize {
                // vec * mat
                let mut out = [0u32; 4];
                for col in 0..*cols as usize {
                    let mut s = 0.0;
                    for row in 0..*rows as usize {
                        s += lane_f32(*k, c[row]) * d[col * *rows as usize + row];
                    }
                    out[col] = s.to_bits();
                }
                return Ok(V::Vec {
                    k: Scalar::Float,
                    n: *cols,
                    c: out,
                });
            }
            if *n == 1 {
                let s = lane_f32(*k, c[0]);
                let mut out = [0.0f32; 16];
                for i in 0..(*cols * *rows) as usize {
                    out[i] = float_op(op, s, d[i])?;
                }
                return Ok(V::Mat {
                    cols: *cols,
                    rows: *rows,
                    d: out,
                });
            }
            Err("vector and matrix sizes do not match".into())
        }
        _ => Err("operands cannot be combined".into()),
    }
}

pub fn negate(v: &V) -> R<V> {
    match v {
        V::Vec { k, n, c } => {
            let mut out = [0u32; 4];
            for i in 0..*n as usize {
                out[i] = match k {
                    Scalar::Float => (-f32::from_bits(c[i])).to_bits(),
                    Scalar::Int => (c[i] as i32).wrapping_neg() as u32,
                    Scalar::UInt => c[i].wrapping_neg(),
                    Scalar::Bool => return Err("cannot negate a bool".into()),
                };
            }
            Ok(V::Vec {
                k: *k,
                n: *n,
                c: out,
            })
        }
        V::Mat { cols, rows, d } => {
            let mut out = *d;
            for x in out.iter_mut() {
                *x = -*x;
            }
            Ok(V::Mat {
                cols: *cols,
                rows: *rows,
                d: out,
            })
        }
        _ => Err("cannot negate this".into()),
    }
}

pub fn logical_not(v: &V) -> R<V> {
    match v {
        V::Vec {
            k: Scalar::Bool,
            n,
            c,
        } => {
            let mut out = [0u32; 4];
            for i in 0..*n as usize {
                out[i] = u32::from(c[i] == 0);
            }
            Ok(V::Vec {
                k: Scalar::Bool,
                n: *n,
                c: out,
            })
        }
        _ => Err("! needs a bool".into()),
    }
}

pub fn bit_not(v: &V) -> R<V> {
    match v {
        V::Vec {
            k: k @ (Scalar::Int | Scalar::UInt),
            n,
            c,
        } => {
            let mut out = [0u32; 4];
            for i in 0..*n as usize {
                out[i] = !c[i];
            }
            Ok(V::Vec {
                k: *k,
                n: *n,
                c: out,
            })
        }
        _ => Err("~ needs an integer".into()),
    }
}

/// Component indices of a swizzle (`xyzw`, `rgba` or `stpq` sets).
/// The lanes a swizzle names, without allocating: the first `len` entries of the array.
pub fn swizzle_lanes(name: &str, n: usize) -> Option<([usize; 4], usize)> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 4 {
        return None;
    }
    let set = match bytes[0] {
        b'x' | b'y' | b'z' | b'w' => b"xyzw",
        b'r' | b'g' | b'b' | b'a' => b"rgba",
        b's' | b't' | b'p' | b'q' => b"stpq",
        _ => return None,
    };
    let mut out = [0usize; 4];
    for (slot, c) in out.iter_mut().zip(bytes) {
        let i = set.iter().position(|s| s == c)?;
        if i >= n {
            return None;
        }
        *slot = i;
    }
    Some((out, bytes.len()))
}

pub fn swizzle_indices(name: &str, n: usize) -> Option<Vec<usize>> {
    if name.is_empty() || name.len() > 4 {
        return None;
    }
    let sets = ["xyzw", "rgba", "stpq"];
    for set in sets {
        let idx: Option<Vec<usize>> = name.chars().map(|c| set.find(c)).collect();
        if let Some(idx) = idx {
            return idx.iter().all(|&i| i < n).then_some(idx);
        }
    }
    None
}

pub fn swizzle(v: &V, indices: &[usize]) -> R<V> {
    let V::Vec { k, c, .. } = v else {
        return Err("only vectors can be swizzled".into());
    };
    let mut out = [0u32; 4];
    for (slot, &i) in out.iter_mut().zip(indices) {
        *slot = c[i];
    }
    Ok(V::Vec {
        k: *k,
        n: indices.len() as u8,
        c: out,
    })
}

/// Reads `v[i]`: a lane of a vector, a column of a matrix, an array element.
pub fn index(v: &V, i: i32) -> R<V> {
    match v {
        V::Vec { k, n, c } => {
            let i = i.clamp(0, *n as i32 - 1) as usize;
            Ok(V::Vec {
                k: *k,
                n: 1,
                c: [c[i], 0, 0, 0],
            })
        }
        V::Mat { cols, rows, d } => {
            let i = i.clamp(0, *cols as i32 - 1) as usize;
            let mut out = [0u32; 4];
            for r in 0..*rows as usize {
                out[r] = d[i * *rows as usize + r].to_bits();
            }
            Ok(V::Vec {
                k: Scalar::Float,
                n: *rows,
                c: out,
            })
        }
        V::Arr(items) => {
            let i = i.clamp(0, items.len() as i32 - 1) as usize;
            Ok(items[i].clone())
        }
        _ => Err("this cannot be indexed".into()),
    }
}

/// Writes `value` into `v[i]`.
pub fn set_index(v: &mut V, i: i32, value: V) -> R<()> {
    match v {
        V::Vec { k, n, c } => {
            if i < 0 || i >= *n as i32 {
                return Ok(());
            }
            c[i as usize] = convert_lane(
                match &value {
                    V::Vec { k, .. } => *k,
                    _ => return Err("bad component".into()),
                },
                match &value {
                    V::Vec { c, .. } => c[0],
                    _ => 0,
                },
                *k,
            );
            Ok(())
        }
        V::Mat { cols, rows, d } => {
            if i < 0 || i >= *cols as i32 {
                return Ok(());
            }
            let V::Vec { k, c, .. } = &value else {
                return Err("a matrix column must be a vector".into());
            };
            for r in 0..*rows as usize {
                d[i as usize * *rows as usize + r] = lane_f32(*k, c[r]);
            }
            Ok(())
        }
        V::Arr(items) => {
            if i >= 0 && (i as usize) < items.len() {
                items[i as usize] = value;
            }
            Ok(())
        }
        _ => Err("this cannot be indexed".into()),
    }
}

/// Writes the lanes of `value` into `v` at the swizzle `indices`.
pub fn set_swizzle(v: &mut V, indices: &[usize], value: &V) -> R<()> {
    let (
        V::Vec { k, c, .. },
        V::Vec {
            k: vk,
            c: vc,
            n: vn,
        },
    ) = (&mut *v, value)
    else {
        return Err("only vectors can be swizzled".into());
    };
    for (j, &i) in indices.iter().enumerate() {
        let src = if *vn == 1 { 0 } else { j };
        c[i] = convert_lane(*vk, vc[src], *k);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_and_vector_arithmetic_broadcast_and_convert() {
        let v = binary(BinOp::Mul, &V::vec(&[1.0, 2.0, 3.0]), &V::f(2.0)).unwrap();
        assert_eq!(v.floats().unwrap(), vec![2.0, 4.0, 6.0]);
        let v = binary(BinOp::Add, &V::f(1.0), &V::i(2)).unwrap();
        assert_eq!(v.as_f32(), Some(3.0), "int mixes into float");
        assert_eq!(
            binary(BinOp::Div, &V::i(7), &V::i(2)).unwrap().as_i32(),
            Some(3)
        );
        assert_eq!(
            binary(BinOp::Div, &V::i(7), &V::i(0)).unwrap().as_i32(),
            Some(0),
            "no panic"
        );
        assert_eq!(binary(BinOp::Shl, &V::u(1), &V::u(4)).unwrap(), V::u(16));
        assert!(binary(BinOp::Add, &V::vec(&[1.0, 2.0]), &V::vec(&[1.0, 2.0, 3.0])).is_err());
    }

    #[test]
    fn integer_arithmetic_wraps() {
        assert_eq!(
            binary(BinOp::Add, &V::u(u32::MAX), &V::u(1)).unwrap(),
            V::u(0)
        );
        assert_eq!(
            binary(BinOp::Mul, &V::i(i32::MAX), &V::i(2))
                .unwrap()
                .as_i32(),
            Some(-2)
        );
    }

    #[test]
    fn comparisons_and_equality() {
        assert_eq!(binary(BinOp::Lt, &V::f(1.0), &V::i(2)).unwrap(), V::b(true));
        assert_eq!(binary(BinOp::Ge, &V::u(3), &V::u(3)).unwrap(), V::b(true));
        assert_eq!(
            binary(BinOp::Eq, &V::vec(&[1.0, 2.0]), &V::vec(&[1.0, 2.0])).unwrap(),
            V::b(true)
        );
        assert_eq!(
            binary(BinOp::Ne, &V::vec(&[1.0, 2.0]), &V::vec(&[1.0, 3.0])).unwrap(),
            V::b(true)
        );
        assert_eq!(
            binary(BinOp::Lt, &V::f(f32::NAN), &V::f(1.0)).unwrap(),
            V::b(false)
        );
    }

    fn mat(cols: u8, rows: u8, d: &[f32]) -> V {
        let mut a = [0.0; 16];
        a[..d.len()].copy_from_slice(d);
        V::Mat { cols, rows, d: a }
    }

    #[test]
    fn matrices_multiply_matrices_and_vectors_column_major() {
        // [[1,3],[2,4]] columns (1,2) and (3,4).
        let m = mat(2, 2, &[1.0, 2.0, 3.0, 4.0]);
        let v = binary(BinOp::Mul, &m, &V::vec(&[1.0, 1.0])).unwrap();
        assert_eq!(v.floats().unwrap(), vec![4.0, 6.0], "m * v");
        let v = binary(BinOp::Mul, &V::vec(&[1.0, 1.0]), &m).unwrap();
        assert_eq!(v.floats().unwrap(), vec![3.0, 7.0], "v * m");
        let id = mat(2, 2, &[1.0, 0.0, 0.0, 1.0]);
        assert_eq!(binary(BinOp::Mul, &m, &id).unwrap(), m);
        let sq = binary(BinOp::Mul, &m, &m).unwrap();
        let V::Mat { d, .. } = sq else { panic!() };
        assert_eq!(&d[..4], &[7.0, 10.0, 15.0, 22.0]);
        let scaled = binary(BinOp::Mul, &m, &V::f(2.0)).unwrap();
        let V::Mat { d, .. } = scaled else { panic!() };
        assert_eq!(&d[..4], &[2.0, 4.0, 6.0, 8.0]);
    }

    #[test]
    fn swizzles_read_and_write() {
        let v = V::vec(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(
            swizzle(&v, &swizzle_indices("wzy", 4).unwrap())
                .unwrap()
                .floats()
                .unwrap(),
            vec![4.0, 3.0, 2.0]
        );
        assert_eq!(swizzle_indices("rgb", 3), Some(vec![0, 1, 2]));
        assert_eq!(
            swizzle_indices("xw", 3),
            None,
            "w is out of range for a vec3"
        );
        assert_eq!(swizzle_indices("xg", 4), None, "sets do not mix");
        let mut v = v;
        set_swizzle(&mut v, &[0, 2], &V::vec(&[9.0, 8.0])).unwrap();
        assert_eq!(v.floats().unwrap(), vec![9.0, 2.0, 8.0, 4.0]);
        set_swizzle(&mut v, &[1, 3], &V::f(0.0)).unwrap();
        assert_eq!(v.floats().unwrap(), vec![9.0, 0.0, 8.0, 0.0]);
    }

    #[test]
    fn indexing_vectors_matrices_and_arrays_clamps_instead_of_panicking() {
        let v = V::vec(&[1.0, 2.0, 3.0]);
        assert_eq!(index(&v, 1).unwrap().as_f32(), Some(2.0));
        assert_eq!(index(&v, 99).unwrap().as_f32(), Some(3.0));
        let m = mat(2, 2, &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(index(&m, 1).unwrap().floats().unwrap(), vec![3.0, 4.0]);
        let mut a = V::Arr(vec![V::f(1.0), V::f(2.0)]);
        set_index(&mut a, 1, V::f(5.0)).unwrap();
        assert_eq!(index(&a, 1).unwrap().as_f32(), Some(5.0));
        set_index(&mut a, 7, V::f(1.0)).unwrap();
    }

    #[test]
    fn coercion_follows_the_declared_type() {
        assert_eq!(coerce(V::i(2), &Ty::FLOAT).as_f32(), Some(2.0));
        assert_eq!(coerce(V::f(2.7), &Ty::INT).as_i32(), Some(2));
        let v = coerce(V::vec(&[1.5, 2.5]), &Ty::Vec(Scalar::Int, 2));
        assert_eq!(v, V::of_kind(Scalar::Int, &[1, 2]));
        assert_eq!(
            coerce(V::f(3.0), &Ty::Vec(Scalar::Float, 3))
                .floats()
                .unwrap(),
            vec![3.0, 3.0, 3.0]
        );
    }

    #[test]
    fn unary_operators() {
        assert_eq!(
            negate(&V::vec(&[1.0, -2.0])).unwrap().floats().unwrap(),
            vec![-1.0, 2.0]
        );
        assert_eq!(logical_not(&V::b(true)).unwrap(), V::b(false));
        assert_eq!(bit_not(&V::u(0)).unwrap(), V::u(u32::MAX));
        assert!(negate(&V::b(true)).is_err());
    }
}
