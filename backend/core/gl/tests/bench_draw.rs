// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Developer measurement, not a regression test: the draw that dominated a real page
//! (Cloudflare's globe: thousands of instanced dots, ~2.4 million vertex-shader runs and
//! ~400,000 triangles per call), reproduced from synthetic geometry.
//!
//! `cargo test -p blueice-gl --release --test bench_draw -- --ignored --nocapture`
//! `BI_GL_PROFILE=1` adds the stage timings; `BI_VERT=a.vert BI_FRAG=b.frag` use shaders a
//! page compiled (see `BI_DUMP_GLSL`) instead of the built-in ones.

use blueice_gl::*;

const VS: &str = "\
#version 300 es
in vec3 position;
in vec3 normal;
in vec2 uv;
in mat4 instanceMatrix;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform mat3 normalMatrix;
out vec2 vUv;
out vec3 vNormal;
void main() {
    vUv = uv;
    vNormal = normalMatrix * mat3(instanceMatrix) * normal;
    vec4 mvPosition = modelViewMatrix * instanceMatrix * vec4(position, 1.0);
    gl_Position = projectionMatrix * mvPosition;
}";

const FS: &str = "\
#version 300 es
precision highp float;
in vec2 vUv;
in vec3 vNormal;
out vec4 fragColor;
uniform vec3 diffuse;
void main() {
    fragColor = vec4(diffuse * (0.5 + 0.5 * normalize(vNormal).z), 1.0);
}";

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[test]
#[ignore = "a measurement; run by hand"]
fn instanced_dots() {
    let vs = std::env::var("BI_VERT")
        .ok()
        .map_or_else(|| VS.to_string(), |p| std::fs::read_to_string(p).unwrap());
    let fs = std::env::var("BI_FRAG")
        .ok()
        .map_or_else(|| FS.to_string(), |p| std::fs::read_to_string(p).unwrap());
    let mut ctx = Context::new(1278, 500);
    ctx.draw_budget = std::time::Duration::from_secs(60);
    let (v, f) = (
        ctx.create_shader(VERTEX_SHADER),
        ctx.create_shader(FRAGMENT_SHADER),
    );
    ctx.shader_source(v, &vs);
    ctx.shader_source(f, &fs);
    ctx.compile_shader(v);
    ctx.compile_shader(f);
    assert!(ctx.shader_compiled(v), "{}", ctx.shader_log(v));
    assert!(ctx.shader_compiled(f), "{}", ctx.shader_log(f));
    let p = ctx.create_program();
    ctx.attach_shader(p, v);
    ctx.attach_shader(p, f);
    ctx.link_program(p);
    assert!(ctx.program_linked(p), "{}", ctx.program_log(p));
    ctx.use_program(p);

    // A disc of 120 vertices drawn as a fan: 119 triangles, 357 indices.
    let rim = 119usize;
    let mut position = vec![0.0f32, 0.0, 0.0];
    let mut normal = vec![0.0f32, 0.0, 1.0];
    let mut uv = vec![0.5f32, 0.5];
    for i in 0..rim {
        let a = i as f32 / rim as f32 * std::f32::consts::TAU;
        position.extend([a.cos() * 0.5, a.sin() * 0.5, 0.0]);
        normal.extend([0.0, 0.0, 1.0]);
        uv.extend([0.5 + a.cos() * 0.5, 0.5 + a.sin() * 0.5]);
    }
    let mut indices: Vec<u8> = Vec::new();
    for i in 0..rim {
        for index in [0u16, 1 + i as u16, 1 + ((i + 1) % rim) as u16] {
            indices.extend(index.to_le_bytes());
        }
    }
    let index_count = (indices.len() / 2) as u32;

    // 6,707 instances scattered over a sphere in front of the camera.
    let instances = 6707usize;
    let mut matrices: Vec<f32> = Vec::new();
    for k in 0..instances {
        let t = k as f32 / instances as f32;
        let (lat, lon) = ((t * 2.0 - 1.0) * 1.2, t * 997.0);
        let (x, y, z) = (lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
        let s = 0.012;
        matrices.extend([
            s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, s, 0.0, x, y, z, 1.0,
        ]);
    }

    let bind = |ctx: &mut Context,
                name: &str,
                bytes: Vec<u8>,
                size: u32,
                stride: u32,
                divisor: u32,
                columns: u32| {
        let buffer = ctx.create_buffer();
        ctx.bind_buffer(ARRAY_BUFFER, buffer);
        ctx.buffer_data(ARRAY_BUFFER, bytes);
        let base = ctx.attrib_location(p, name);
        if base < 0 {
            return;
        }
        for column in 0..columns {
            let loc = base as u32 + column;
            ctx.enable_vertex_attrib_array(loc, true);
            ctx.vertex_attrib_pointer(loc, size, 0x1406, false, stride, column * size * 4, false);
            ctx.vertex_attrib_divisor(loc, divisor);
        }
    };
    bind(&mut ctx, "position", floats(&position), 3, 0, 0, 1);
    bind(&mut ctx, "normal", floats(&normal), 3, 0, 0, 1);
    bind(&mut ctx, "uv", floats(&uv), 2, 0, 0, 1);
    bind(&mut ctx, "instanceMatrix", floats(&matrices), 4, 64, 1, 4);
    let element = ctx.create_buffer();
    ctx.bind_buffer(ELEMENT_ARRAY_BUFFER, element);
    ctx.buffer_data(ELEMENT_ARRAY_BUFFER, indices);

    let set = |ctx: &mut Context, name: &str, values: &[f32], matrix: Option<(usize, usize)>| {
        let loc = ctx.uniform_location(p, name);
        if loc >= 0 {
            ctx.set_uniform(loc, values, None, matrix);
        }
    };
    let f = 1.0 / (45.0f32.to_radians() / 2.0).tan();
    let aspect = 1278.0 / 500.0;
    let (near, far) = (0.1f32, 100.0f32);
    set(
        &mut ctx,
        "projectionMatrix",
        &[
            f / aspect,
            0.0,
            0.0,
            0.0,
            0.0,
            f,
            0.0,
            0.0,
            0.0,
            0.0,
            (far + near) / (near - far),
            -1.0,
            0.0,
            0.0,
            2.0 * far * near / (near - far),
            0.0,
        ],
        Some((4, 4)),
    );
    set(
        &mut ctx,
        "modelViewMatrix",
        &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -3.0, 1.0,
        ],
        Some((4, 4)),
    );
    set(
        &mut ctx,
        "normalMatrix",
        &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        Some((3, 3)),
    );
    set(&mut ctx, "diffuse", &[1.0, 0.4, 0.1], None);
    ctx.state.depth_test = true;
    ctx.state.blend = true;
    ctx.state.blend_src_rgb = 0x302;
    ctx.state.blend_dst_rgb = 0x303;
    ctx.clear(COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT);

    let mut times = Vec::new();
    // `BI_REPEAT=N` repeats the draw, so a sampling profiler has something to see.
    let repeat = std::env::var("BI_REPEAT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    for _ in 0..repeat {
        let start = std::time::Instant::now();
        ctx.draw_elements(0x0004, index_count, 0x1403, 0, instances as u32);
        times.push(start.elapsed());
    }
    assert_eq!(ctx.get_error(), NO_ERROR);
    let covered = ctx.picture().chunks(4).filter(|p| p[3] != 0).count();
    times.sort();
    let (best, median) = (times[0], times[times.len() / 2]);
    // A fingerprint of the picture, to compare implementations pixel for pixel.
    let picture = ctx.picture();
    let fingerprint = picture.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    });
    eprintln!("picture fingerprint {fingerprint:016x}");
    eprintln!(
        "{} vertices per draw; median {:?}, best {:?}; {} pixels drawn",
        instances * index_count as usize,
        median,
        best,
        covered
    );
}
