// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `WebGLRenderingContext` as native calls: one primitive, `glCall`, takes a
//! context id and an operation name and forwards to `blueice-gl`. Binary
//! data (buffers, pixels) travels as base64; arrays of numbers as JSON.

use super::dom_bridge::Reply;
use super::net::{base64_decode, base64_encode};
use blueice_gl::glsl::ast::{SamplerKind, Scalar, Ty};
use blueice_gl::Context;

/// One canvas's context.
pub(crate) struct GlContext {
    pub ctx: Context,
    pub canvas: blueice_dom::NodeId,
    /// Drawn to since the host last took the picture.
    pub dirty: bool,
}

fn num(args: &[Reply], i: usize) -> f64 {
    match args.get(i) {
        Some(Reply::Number(n)) => *n,
        Some(Reply::Bool(b)) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}
fn text(args: &[Reply], i: usize) -> String {
    match args.get(i) {
        Some(Reply::Text(s)) => s.clone(),
        _ => String::new(),
    }
}
fn flag(args: &[Reply], i: usize) -> bool {
    matches!(args.get(i), Some(Reply::Bool(true))) || num(args, i) != 0.0
}

/// The GL enum naming a shader type.
fn gl_type(ty: &Ty) -> u32 {
    match ty {
        Ty::Scalar(Scalar::Float) => 0x1406,
        Ty::Scalar(Scalar::Int) => 0x1404,
        Ty::Scalar(Scalar::UInt) => 0x1405,
        Ty::Scalar(Scalar::Bool) => 0x8B56,
        Ty::Vec(Scalar::Float, n) => 0x8B50 + u32::from(*n) - 2,
        Ty::Vec(Scalar::Int, n) => 0x8B53 + u32::from(*n) - 2,
        Ty::Vec(Scalar::Bool, n) => 0x8B57 + u32::from(*n) - 2,
        Ty::Vec(Scalar::UInt, n) => 0x8DC6 + u32::from(*n) - 2,
        Ty::Mat(2, 2) => 0x8B5A,
        Ty::Mat(3, 3) => 0x8B5B,
        Ty::Mat(4, 4) => 0x8B5C,
        Ty::Sampler(SamplerKind::S2D) => 0x8B5E,
        Ty::Sampler(SamplerKind::Cube) => 0x8B60,
        Ty::Sampler(SamplerKind::S3D) => 0x8B5F,
        Ty::Sampler(SamplerKind::S2DArray) => 0x8DC1,
        Ty::Sampler(SamplerKind::S2DShadow) => 0x8B62,
        Ty::Array(inner, _) => gl_type(inner),
        _ => 0x1406,
    }
}

fn json_f32s(args: &[Reply], i: usize) -> Vec<f32> {
    serde_json::from_str::<Vec<f64>>(&text(args, i))
        .unwrap_or_default()
        .into_iter()
        .map(|x| x as f32)
        .collect()
}

fn json_i32s(args: &[Reply], i: usize) -> Vec<i32> {
    serde_json::from_str::<Vec<f64>>(&text(args, i))
        .unwrap_or_default()
        .into_iter()
        .map(|x| x as i32)
        .collect()
}

pub(crate) fn call(g: &mut GlContext, op: &str, a: &[Reply]) -> Result<Reply, String> {
    let ctx = &mut g.ctx;
    let u = |i: usize| num(a, i) as u32;
    Ok(match op {
        // ---- buffers ----
        "createBuffer" => Reply::Number(f64::from(ctx.create_buffer())),
        "deleteBuffer" => {
            ctx.delete_buffer(u(0));
            Reply::Undefined
        }
        "bindBuffer" => {
            ctx.bind_buffer(u(0), u(1));
            Reply::Undefined
        }
        "bufferData" => {
            let data = base64_decode(&text(a, 1)).ok_or("bad buffer data")?;
            if data.len() > 256 * 1024 * 1024 {
                return Err("buffer is too large".into());
            }
            ctx.buffer_data(u(0), data);
            Reply::Undefined
        }
        "bufferDataSize" => {
            let size = num(a, 1) as usize;
            if size > 256 * 1024 * 1024 {
                return Err("buffer is too large".into());
            }
            ctx.buffer_data(u(0), vec![0; size]);
            Reply::Undefined
        }
        "bufferSubData" => {
            let data = base64_decode(&text(a, 2)).ok_or("bad buffer data")?;
            ctx.buffer_sub_data(u(0), num(a, 1) as usize, &data);
            Reply::Undefined
        }
        // ---- vertex arrays and attributes ----
        "createVertexArray" => Reply::Number(f64::from(ctx.create_vertex_array())),
        "deleteVertexArray" => {
            ctx.delete_vertex_array(u(0));
            Reply::Undefined
        }
        "bindVertexArray" => {
            ctx.bind_vertex_array(u(0));
            Reply::Undefined
        }
        "enableVertexAttribArray" => {
            ctx.enable_vertex_attrib_array(u(0), true);
            Reply::Undefined
        }
        "disableVertexAttribArray" => {
            ctx.enable_vertex_attrib_array(u(0), false);
            Reply::Undefined
        }
        "vertexAttribPointer" => {
            ctx.vertex_attrib_pointer(u(0), u(1), u(2), flag(a, 3), u(4), u(5), flag(a, 6));
            Reply::Undefined
        }
        "vertexAttribDivisor" => {
            ctx.vertex_attrib_divisor(u(0), u(1));
            Reply::Undefined
        }
        "vertexAttrib" => {
            let v = json_f32s(a, 1);
            let mut c = [0.0, 0.0, 0.0, 1.0];
            for (slot, x) in c.iter_mut().zip(v) {
                *slot = x;
            }
            ctx.vertex_attrib_constant(u(0), c);
            Reply::Undefined
        }
        // ---- shaders and programs ----
        "createShader" => Reply::Number(f64::from(ctx.create_shader(u(0)))),
        "shaderSource" => {
            ctx.shader_source(u(0), &text(a, 1));
            Reply::Undefined
        }
        "compileShader" => {
            ctx.compile_shader(u(0));
            // Developer aid: BI_DUMP_GLSL=<dir> keeps every shader a page compiles.
            if let Ok(dir) = std::env::var("BI_DUMP_GLSL") {
                let kind = if ctx.shader_source_of(u(0)).contains("gl_Position") {
                    "vert"
                } else {
                    "frag"
                };
                let verdict = if ctx.shader_compiled(u(0)) {
                    "ok".to_string()
                } else {
                    ctx.shader_log(u(0))
                };
                let name = format!(
                    "{dir}/shader-{}-{}.{kind}",
                    u(0),
                    if verdict == "ok" { "ok" } else { "FAIL" }
                );
                let _ = std::fs::write(
                    &name,
                    format!("// {verdict}\n{}", ctx.shader_source_of(u(0))),
                );
            }
            Reply::Undefined
        }
        "shaderCompiled" => Reply::Bool(ctx.shader_compiled(u(0))),
        "shaderLog" => Reply::Text(ctx.shader_log(u(0))),
        "shaderSourceOf" => Reply::Text(ctx.shader_source_of(u(0))),
        "deleteShader" => {
            ctx.delete_shader(u(0));
            Reply::Undefined
        }
        "createProgram" => Reply::Number(f64::from(ctx.create_program())),
        "attachShader" => {
            ctx.attach_shader(u(0), u(1));
            Reply::Undefined
        }
        "detachShader" => {
            ctx.detach_shader(u(0), u(1));
            Reply::Undefined
        }
        "linkProgram" => {
            ctx.link_program(u(0));
            Reply::Undefined
        }
        "programLinked" => Reply::Bool(ctx.program_linked(u(0))),
        "programLog" => Reply::Text(ctx.program_log(u(0))),
        "useProgram" => {
            ctx.use_program(u(0));
            Reply::Undefined
        }
        "deleteProgram" => {
            ctx.delete_program(u(0));
            Reply::Undefined
        }
        "bindAttribLocation" => {
            ctx.bind_attrib_location(u(0), u(1), &text(a, 2));
            Reply::Undefined
        }
        "attribLocation" => Reply::Number(f64::from(ctx.attrib_location(u(0), &text(a, 1)))),
        "uniformLocation" => Reply::Number(f64::from(ctx.uniform_location(u(0), &text(a, 1)))),
        "activeUniforms" => {
            let list: Vec<String> = ctx
                .active_uniforms(u(0))
                .iter()
                .map(|x| {
                    format!(
                        "{{\"name\":{},\"type\":{},\"size\":{}}}",
                        json(&x.name),
                        gl_type(&x.ty),
                        x.size
                    )
                })
                .collect();
            Reply::Text(format!("[{}]", list.join(",")))
        }
        "activeAttribs" => {
            let list: Vec<String> = ctx
                .active_attribs(u(0))
                .iter()
                .map(|x| {
                    format!(
                        "{{\"name\":{},\"type\":{},\"size\":1,\"location\":{}}}",
                        json(&x.name),
                        gl_type(&x.ty),
                        x.location
                    )
                })
                .collect();
            Reply::Text(format!("[{}]", list.join(",")))
        }
        // `kind` is "f", "i" or "m<cols>x<rows>"; `values` JSON numbers.
        "uniform" => {
            let kind = text(a, 1);
            if let Some(dims) = kind.strip_prefix('m') {
                let (c, r) = dims.split_once('x').unwrap_or(("4", "4"));
                ctx.set_uniform(
                    num(a, 0) as i32,
                    &json_f32s(a, 2),
                    None,
                    Some((c.parse().unwrap_or(4), r.parse().unwrap_or(4))),
                );
            } else if kind == "i" {
                let ints = json_i32s(a, 2);
                ctx.set_uniform(num(a, 0) as i32, &[], Some(&ints), None);
            } else {
                ctx.set_uniform(num(a, 0) as i32, &json_f32s(a, 2), None, None);
            }
            Reply::Undefined
        }
        // ---- textures ----
        "createTexture" => Reply::Number(f64::from(ctx.create_texture())),
        "deleteTexture" => {
            ctx.delete_texture(u(0));
            Reply::Undefined
        }
        "activeTexture" => {
            ctx.active_texture(u(0).wrapping_sub(0x84C0));
            Reply::Undefined
        }
        "bindTexture" => {
            ctx.bind_texture(u(0), u(1));
            Reply::Undefined
        }
        "texParameter" => {
            ctx.tex_parameter(u(0), u(1), u(2));
            Reply::Undefined
        }
        "texImage2D" => {
            let bytes = match a.get(7) {
                Some(Reply::Text(s)) if !s.is_empty() => {
                    Some(base64_decode(s).ok_or("bad pixel data")?)
                }
                _ => None,
            };
            ctx.tex_image_2d(u(0), u(1), u(2), u(3), u(4), u(5), u(6), bytes.as_deref());
            Reply::Undefined
        }
        "texSubImage2D" => {
            let bytes = base64_decode(&text(a, 8)).ok_or("bad pixel data")?;
            ctx.tex_sub_image_2d(u(0), u(1), u(2), u(3), u(4), u(5), u(6), u(7), &bytes);
            Reply::Undefined
        }
        "generateMipmap" => {
            ctx.generate_mipmap(u(0));
            Reply::Undefined
        }
        "pixelStore" => {
            match u(0) {
                0x9240 => ctx.flip_y = flag(a, 1),
                0x9241 => ctx.premultiply = flag(a, 1),
                _ => {}
            }
            Reply::Undefined
        }
        // ---- framebuffers ----
        "createFramebuffer" => Reply::Number(f64::from(ctx.create_framebuffer())),
        "deleteFramebuffer" => {
            ctx.delete_framebuffer(u(0));
            Reply::Undefined
        }
        "bindFramebuffer" => {
            ctx.bind_framebuffer(u(0), u(1));
            Reply::Undefined
        }
        "createRenderbuffer" => Reply::Number(f64::from(ctx.create_renderbuffer())),
        "deleteRenderbuffer" => {
            ctx.delete_renderbuffer(u(0));
            Reply::Undefined
        }
        "bindRenderbuffer" => {
            ctx.bind_renderbuffer(u(1));
            Reply::Undefined
        }
        "renderbufferStorage" => {
            ctx.renderbuffer_storage(u(0), u(1), u(2));
            Reply::Undefined
        }
        "framebufferTexture2D" => {
            let target = u(3);
            let face = if (blueice_gl::TEXTURE_CUBE_MAP_POSITIVE_X
                ..blueice_gl::TEXTURE_CUBE_MAP_POSITIVE_X + 6)
                .contains(&u(1))
            {
                (u(1) - blueice_gl::TEXTURE_CUBE_MAP_POSITIVE_X) as usize
            } else {
                0
            };
            ctx.framebuffer_texture(u(0), target, face, u(4) as usize);
            Reply::Undefined
        }
        "framebufferRenderbuffer" => {
            ctx.framebuffer_renderbuffer(u(0), u(1));
            Reply::Undefined
        }
        "checkFramebufferStatus" => Reply::Number(f64::from(ctx.check_framebuffer_status())),
        "readPixels" => {
            let px = ctx.read_pixels(num(a, 0) as i32, num(a, 1) as i32, u(2), u(3));
            Reply::Text(base64_encode(&px))
        }
        // ---- state ----
        "enable" => {
            ctx.enable(u(0), true);
            Reply::Undefined
        }
        "disable" => {
            ctx.enable(u(0), false);
            Reply::Undefined
        }
        "isEnabled" => Reply::Bool(ctx.is_enabled(u(0))),
        "viewport" => {
            ctx.state.viewport = [
                num(a, 0) as i32,
                num(a, 1) as i32,
                num(a, 2) as i32,
                num(a, 3) as i32,
            ];
            Reply::Undefined
        }
        "scissor" => {
            ctx.state.scissor = [
                num(a, 0) as i32,
                num(a, 1) as i32,
                num(a, 2) as i32,
                num(a, 3) as i32,
            ];
            Reply::Undefined
        }
        "clearColor" => {
            ctx.state.clear_color = [
                num(a, 0) as f32,
                num(a, 1) as f32,
                num(a, 2) as f32,
                num(a, 3) as f32,
            ];
            Reply::Undefined
        }
        "clearDepth" => {
            ctx.state.clear_depth = num(a, 0) as f32;
            Reply::Undefined
        }
        "clear" => {
            ctx.clear(u(0));
            g.dirty = true;
            Reply::Undefined
        }
        "clearBuffer" => {
            // clearBufferfv(COLOR, 0, [r,g,b,a]) / clearBufferfv(DEPTH, 0, [d]).
            let v = json_f32s(a, 1);
            let saved = ctx.state.clone();
            match u(0) {
                0x1800 if v.len() >= 4 => {
                    ctx.state.clear_color = [v[0], v[1], v[2], v[3]];
                    ctx.clear(blueice_gl::COLOR_BUFFER_BIT);
                }
                0x1801 if !v.is_empty() => {
                    ctx.state.clear_depth = v[0];
                    ctx.clear(blueice_gl::DEPTH_BUFFER_BIT);
                }
                _ => {}
            }
            ctx.state = saved;
            g.dirty = true;
            Reply::Undefined
        }
        "colorMask" => {
            ctx.state.color_mask = [flag(a, 0), flag(a, 1), flag(a, 2), flag(a, 3)];
            Reply::Undefined
        }
        "depthMask" => {
            ctx.state.depth_mask = flag(a, 0);
            Reply::Undefined
        }
        "depthFunc" => {
            ctx.state.depth_func = u(0);
            Reply::Undefined
        }
        "depthRange" => {
            ctx.state.depth_range = [num(a, 0) as f32, num(a, 1) as f32];
            Reply::Undefined
        }
        "blendFunc" => {
            let (s, d) = (u(0), u(1));
            ctx.state.blend_src_rgb = s;
            ctx.state.blend_src_alpha = s;
            ctx.state.blend_dst_rgb = d;
            ctx.state.blend_dst_alpha = d;
            Reply::Undefined
        }
        "blendFuncSeparate" => {
            ctx.state.blend_src_rgb = u(0);
            ctx.state.blend_dst_rgb = u(1);
            ctx.state.blend_src_alpha = u(2);
            ctx.state.blend_dst_alpha = u(3);
            Reply::Undefined
        }
        "blendEquation" => {
            ctx.state.blend_eq_rgb = u(0);
            ctx.state.blend_eq_alpha = u(0);
            Reply::Undefined
        }
        "blendEquationSeparate" => {
            ctx.state.blend_eq_rgb = u(0);
            ctx.state.blend_eq_alpha = u(1);
            Reply::Undefined
        }
        "blendColor" => {
            ctx.state.blend_color = [
                num(a, 0) as f32,
                num(a, 1) as f32,
                num(a, 2) as f32,
                num(a, 3) as f32,
            ];
            Reply::Undefined
        }
        "cullFace" => {
            ctx.state.cull_mode = u(0);
            Reply::Undefined
        }
        "frontFace" => {
            ctx.state.front_face = u(0);
            Reply::Undefined
        }
        "lineWidth" => {
            ctx.state.line_width = num(a, 0) as f32;
            Reply::Undefined
        }
        "viewportState" => {
            let v = ctx.state.viewport;
            Reply::Text(format!("[{},{},{},{}]", v[0], v[1], v[2], v[3]))
        }
        "getError" => Reply::Number(f64::from(ctx.get_error())),
        // ---- drawing ----
        "drawArrays" => {
            let started = std::time::Instant::now();
            ctx.draw_arrays(u(0), u(1), u(2), u(3).max(1));
            if std::env::var("BI_DIAG").is_ok() {
                eprintln!(
                    "DIAG gl drawArrays {}x{} took {:?} (slow: {})",
                    ctx.width,
                    ctx.height,
                    started.elapsed(),
                    ctx.slow_draws
                );
            }
            g.dirty = true;
            Reply::Undefined
        }
        "drawElements" => {
            let started = std::time::Instant::now();
            ctx.draw_elements(u(0), u(1), u(2), u(3), u(4).max(1));
            if std::env::var("BI_DIAG").is_ok() {
                eprintln!(
                    "DIAG gl drawElements {}x{} took {:?} (slow: {})",
                    ctx.width,
                    ctx.height,
                    started.elapsed(),
                    ctx.slow_draws
                );
            }
            g.dirty = true;
            Reply::Undefined
        }
        "resize" => {
            ctx.resize(u(0).clamp(1, 8192), u(1).clamp(1, 8192));
            g.dirty = true;
            Reply::Undefined
        }
        other => return Err(format!("unknown GL operation '{other}'")),
    })
}

fn json(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Reply {
        Reply::Text(s.to_string())
    }
    fn n(x: f64) -> Reply {
        Reply::Number(x)
    }

    fn context() -> GlContext {
        GlContext {
            ctx: Context::new(8, 8),
            canvas: blueice_dom::NodeId::from_u64(1),
            dirty: false,
        }
    }

    fn num_of(r: Reply) -> f64 {
        match r {
            Reply::Number(n) => n,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_program_compiles_links_and_draws_through_the_bridge() {
        let mut g = context();
        let vs = num_of(
            call(
                &mut g,
                "createShader",
                &[n(f64::from(blueice_gl::VERTEX_SHADER))],
            )
            .unwrap(),
        );
        let fs = num_of(
            call(
                &mut g,
                "createShader",
                &[n(f64::from(blueice_gl::FRAGMENT_SHADER))],
            )
            .unwrap(),
        );
        call(
            &mut g,
            "shaderSource",
            &[
                n(vs),
                t("attribute vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }"),
            ],
        )
        .unwrap();
        call(&mut g, "shaderSource", &[n(fs), t("precision mediump float; uniform vec4 tint; void main() { gl_FragColor = tint; }")]).unwrap();
        for s in [vs, fs] {
            call(&mut g, "compileShader", &[n(s)]).unwrap();
            assert_eq!(
                call(&mut g, "shaderCompiled", &[n(s)]).unwrap(),
                Reply::Bool(true)
            );
        }
        let p = num_of(call(&mut g, "createProgram", &[]).unwrap());
        for s in [vs, fs] {
            call(&mut g, "attachShader", &[n(p), n(s)]).unwrap();
        }
        call(&mut g, "linkProgram", &[n(p)]).unwrap();
        assert_eq!(
            call(&mut g, "programLinked", &[n(p)]).unwrap(),
            Reply::Bool(true)
        );
        call(&mut g, "useProgram", &[n(p)]).unwrap();
        let tint = num_of(call(&mut g, "uniformLocation", &[n(p), t("tint")]).unwrap());
        call(&mut g, "uniform", &[n(tint), t("f"), t("[0, 1, 0, 1]")]).unwrap();
        let loc = num_of(call(&mut g, "attribLocation", &[n(p), t("p")]).unwrap());
        let buffer = num_of(call(&mut g, "createBuffer", &[]).unwrap());
        call(
            &mut g,
            "bindBuffer",
            &[n(f64::from(blueice_gl::ARRAY_BUFFER)), n(buffer)],
        )
        .unwrap();
        let verts: Vec<u8> = [-1.0f32, -1.0, 3.0, -1.0, -1.0, 3.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        call(
            &mut g,
            "bufferData",
            &[
                n(f64::from(blueice_gl::ARRAY_BUFFER)),
                t(&base64_encode(&verts)),
                n(35044.0),
            ],
        )
        .unwrap();
        call(&mut g, "enableVertexAttribArray", &[n(loc)]).unwrap();
        call(
            &mut g,
            "vertexAttribPointer",
            &[
                n(loc),
                n(2.0),
                n(5126.0),
                Reply::Bool(false),
                n(0.0),
                n(0.0),
            ],
        )
        .unwrap();
        call(&mut g, "drawArrays", &[n(4.0), n(0.0), n(3.0)]).unwrap();
        assert!(g.dirty);
        assert_eq!(num_of(call(&mut g, "getError", &[]).unwrap()), 0.0);
        let Reply::Text(px) =
            call(&mut g, "readPixels", &[n(0.0), n(0.0), n(1.0), n(1.0)]).unwrap()
        else {
            panic!()
        };
        assert_eq!(base64_decode(&px).unwrap(), vec![0, 255, 0, 255]);
    }

    #[test]
    fn active_uniforms_and_attributes_are_described_with_gl_types() {
        let mut g = context();
        let vs = num_of(
            call(
                &mut g,
                "createShader",
                &[n(f64::from(blueice_gl::VERTEX_SHADER))],
            )
            .unwrap(),
        );
        let fs = num_of(
            call(
                &mut g,
                "createShader",
                &[n(f64::from(blueice_gl::FRAGMENT_SHADER))],
            )
            .unwrap(),
        );
        call(&mut g, "shaderSource", &[n(vs), t("attribute vec3 position; uniform mat4 m; uniform vec2 offsets[4]; void main() { gl_Position = m * vec4(position, 1.0) + vec4(offsets[0], 0.0, 0.0); }")]).unwrap();
        call(&mut g, "shaderSource", &[n(fs), t("uniform sampler2D map; void main() { gl_FragColor = texture2D(map, vec2(0.5)); }")]).unwrap();
        for s in [vs, fs] {
            call(&mut g, "compileShader", &[n(s)]).unwrap();
        }
        let p = num_of(call(&mut g, "createProgram", &[]).unwrap());
        for s in [vs, fs] {
            call(&mut g, "attachShader", &[n(p), n(s)]).unwrap();
        }
        call(&mut g, "linkProgram", &[n(p)]).unwrap();
        let Reply::Text(u) = call(&mut g, "activeUniforms", &[n(p)]).unwrap() else {
            panic!()
        };
        assert!(
            u.contains("\"name\":\"m\",\"type\":35676,\"size\":1"),
            "{u}"
        );
        assert!(
            u.contains("\"name\":\"offsets[0]\",\"type\":35664,\"size\":4"),
            "{u}"
        );
        assert!(u.contains("\"name\":\"map\",\"type\":35678"), "{u}");
        let Reply::Text(attribs) = call(&mut g, "activeAttribs", &[n(p)]).unwrap() else {
            panic!()
        };
        assert!(
            attribs.contains("\"name\":\"position\",\"type\":35665"),
            "{attribs}"
        );
    }

    #[test]
    fn unknown_operations_and_bad_data_are_errors() {
        let mut g = context();
        assert!(call(&mut g, "frobnicate", &[]).is_err());
        assert!(call(&mut g, "bufferData", &[n(34962.0), t("@@"), n(0.0)]).is_err());
        assert!(call(&mut g, "bufferDataSize", &[n(34962.0), n(1e12)]).is_err());
    }

    #[test]
    fn resize_clears_and_marks_the_canvas_for_repaint() {
        let mut g = context();
        call(&mut g, "clearColor", &[n(1.0), n(0.0), n(0.0), n(1.0)]).unwrap();
        call(&mut g, "clear", &[n(16384.0)]).unwrap();
        g.dirty = false;
        call(&mut g, "resize", &[n(4.0), n(2.0)]).unwrap();
        assert!(g.dirty);
        assert_eq!((g.ctx.width, g.ctx.height), (4, 2));
        assert_eq!(&g.ctx.picture()[..4], &[0, 0, 0, 0]);
    }
}
