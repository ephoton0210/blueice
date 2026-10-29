// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_gl::*;

const VS: &str = "attribute vec2 position; attribute vec4 color; varying vec4 vColor;\n\
    void main() { vColor = color; gl_Position = vec4(position, 0.0, 1.0); }";
const FS: &str =
    "precision mediump float; varying vec4 vColor; void main() { gl_FragColor = vColor; }";

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn build(ctx: &mut Context, vs: &str, fs: &str) -> u32 {
    let (v, f) = (
        ctx.create_shader(VERTEX_SHADER),
        ctx.create_shader(FRAGMENT_SHADER),
    );
    ctx.shader_source(v, vs);
    ctx.shader_source(f, fs);
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
    p
}

fn attrib(ctx: &mut Context, p: u32, name: &str, size: u32, stride: u32, offset: u32) {
    let loc = ctx.attrib_location(p, name);
    assert!(loc >= 0, "{name} has no location");
    ctx.enable_vertex_attrib_array(loc as u32, true);
    ctx.vertex_attrib_pointer(loc as u32, size, 0x1406, false, stride, offset, false);
}

fn pixel(ctx: &Context, x: u32, y_from_top: u32) -> [u8; 4] {
    let pic = ctx.picture();
    let i = ((y_from_top * ctx.width + x) * 4) as usize;
    [pic[i], pic[i + 1], pic[i + 2], pic[i + 3]]
}

/// Two floats of position and four of colour per vertex.
fn interleaved(ctx: &mut Context, p: u32, verts: &[[f32; 6]]) {
    let b = ctx.create_buffer();
    ctx.bind_buffer(ARRAY_BUFFER, b);
    let flat: Vec<f32> = verts.iter().flatten().copied().collect();
    ctx.buffer_data(ARRAY_BUFFER, floats(&flat));
    attrib(ctx, p, "position", 2, 24, 0);
    attrib(ctx, p, "color", 4, 24, 8);
}

#[test]
fn a_triangle_is_rasterized_with_interpolated_colour() {
    let mut ctx = Context::new(32, 32);
    ctx.state.clear_color = [0.0, 0.0, 0.0, 1.0];
    ctx.clear(COLOR_BUFFER_BIT);
    let p = build(&mut ctx, VS, FS);
    interleaved(
        &mut ctx,
        p,
        &[
            [-1.0, -1.0, 1.0, 0.0, 0.0, 1.0],
            [1.0, -1.0, 0.0, 1.0, 0.0, 1.0],
            [-1.0, 1.0, 0.0, 0.0, 1.0, 1.0],
        ],
    );
    ctx.draw_arrays(0x0004, 0, 3, 1);
    assert_eq!(ctx.get_error(), NO_ERROR);
    // The lower-left corner (bottom row of the picture) is nearly pure red.
    let near_red = pixel(&ctx, 1, 30);
    assert!(
        near_red[0] > 200 && near_red[1] < 40 && near_red[2] < 40,
        "{near_red:?}"
    );
    // The upper-left is nearly pure blue; the far corner is outside the triangle.
    let near_blue = pixel(&ctx, 1, 4);
    assert!(near_blue[2] > 200 && near_blue[0] < 40, "{near_blue:?}");
    assert_eq!(pixel(&ctx, 30, 1), [0, 0, 0, 255], "outside stays cleared");
    // Colours blend inside.
    let mid = pixel(&ctx, 6, 12);
    assert!(mid[0] > 20 && mid[1] > 20 && mid[2] > 20, "{mid:?}");
}

#[test]
fn clear_and_read_pixels_use_the_clear_colour_and_masks() {
    let mut ctx = Context::new(4, 4);
    ctx.state.clear_color = [1.0, 0.5, 0.0, 1.0];
    ctx.clear(COLOR_BUFFER_BIT);
    let px = ctx.read_pixels(0, 0, 1, 1);
    assert_eq!(px, vec![255, 128, 0, 255]);
    ctx.state.color_mask = [false, true, false, true];
    ctx.state.clear_color = [0.0; 4];
    ctx.clear(COLOR_BUFFER_BIT);
    assert_eq!(ctx.read_pixels(0, 0, 1, 1), vec![255, 0, 0, 0]);
}

#[test]
fn depth_testing_keeps_the_nearer_triangle_whatever_the_draw_order() {
    let vs = "attribute vec3 position; uniform vec4 tint; varying vec4 vColor; void main() { vColor = tint; gl_Position = vec4(position, 1.0); }";
    let mut ctx = Context::new(16, 16);
    let p = build(&mut ctx, vs, FS);
    ctx.state.depth_test = true;
    ctx.clear(COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT);
    let b = ctx.create_buffer();
    ctx.bind_buffer(ARRAY_BUFFER, b);
    let tri = |z: f32| [-1.0, -1.0, z, 3.0, -1.0, z, -1.0, 3.0, z];
    let loc = ctx.attrib_location(p, "position") as u32;
    ctx.enable_vertex_attrib_array(loc, true);
    let tint = ctx.uniform_location(p, "tint");
    let draw = |ctx: &mut Context, z: f32, rgba: [f32; 4]| {
        ctx.buffer_data(ARRAY_BUFFER, floats(&tri(z)));
        ctx.vertex_attrib_pointer(loc, 3, 0x1406, false, 0, 0, false);
        ctx.set_uniform(tint, &rgba, None, None);
        ctx.draw_arrays(0x0004, 0, 3, 1);
    };
    draw(&mut ctx, 0.5, [0.0, 0.0, 1.0, 1.0]); // far, blue
    draw(&mut ctx, -0.5, [1.0, 0.0, 0.0, 1.0]); // near, red
    draw(&mut ctx, 0.0, [0.0, 1.0, 0.0, 1.0]); // middle, green: loses to red
    assert_eq!(pixel(&ctx, 8, 8), [255, 0, 0, 255]);
}

#[test]
fn blending_mixes_with_the_framebuffer_and_culling_drops_back_faces() {
    let mut ctx = Context::new(8, 8);
    ctx.state.clear_color = [0.0, 0.0, 1.0, 1.0];
    ctx.clear(COLOR_BUFFER_BIT);
    let p = build(&mut ctx, VS, FS);
    interleaved(
        &mut ctx,
        p,
        &[
            [-1.0, -1.0, 1.0, 0.0, 0.0, 0.5],
            [3.0, -1.0, 1.0, 0.0, 0.0, 0.5],
            [-1.0, 3.0, 1.0, 0.0, 0.0, 0.5],
        ],
    );
    ctx.state.blend = true;
    ctx.state.blend_src_rgb = 0x302;
    ctx.state.blend_dst_rgb = 0x303;
    ctx.state.blend_src_alpha = 1;
    ctx.state.blend_dst_alpha = 0x303;
    ctx.draw_arrays(0x0004, 0, 3, 1);
    let mixed = pixel(&ctx, 4, 4);
    assert!(
        (i32::from(mixed[0]) - 128).abs() <= 2 && (i32::from(mixed[2]) - 128).abs() <= 2,
        "{mixed:?}"
    );
    // The same triangle is counter-clockwise (front); culling front faces removes it.
    ctx.state.clear_color = [0.0, 0.0, 0.0, 1.0];
    ctx.clear(COLOR_BUFFER_BIT);
    ctx.state.blend = false;
    ctx.state.cull = true;
    ctx.state.cull_mode = 0x0404;
    ctx.draw_arrays(0x0004, 0, 3, 1);
    assert_eq!(pixel(&ctx, 4, 4), [0, 0, 0, 255]);
    ctx.state.cull_mode = 0x0405;
    ctx.draw_arrays(0x0004, 0, 3, 1);
    assert_eq!(pixel(&ctx, 4, 4)[0], 255);
}

#[test]
fn draw_elements_strips_fans_and_instances() {
    let vs = "attribute vec2 position; uniform float shift; void main() { gl_Position = vec4(position.x + shift * float(gl_InstanceID), position.y, 0.0, 1.0); }";
    let fs = "void main() { gl_FragColor = vec4(1.0, 1.0, 1.0, 1.0); }";
    let mut ctx = Context::new(16, 16);
    let p = build(&mut ctx, vs, fs);
    let b = ctx.create_buffer();
    ctx.bind_buffer(ARRAY_BUFFER, b);
    // A small square (two triangles by index) in the lower left.
    ctx.buffer_data(
        ARRAY_BUFFER,
        floats(&[-1.0, -1.0, -0.5, -1.0, -0.5, -0.5, -1.0, -0.5]),
    );
    attrib(&mut ctx, p, "position", 2, 0, 0);
    let ib = ctx.create_buffer();
    ctx.bind_buffer(ELEMENT_ARRAY_BUFFER, ib);
    ctx.buffer_data(
        ELEMENT_ARRAY_BUFFER,
        vec![0, 0, 1, 0, 2, 0, 0, 0, 2, 0, 3, 0],
    );
    ctx.set_uniform(ctx.uniform_location(p, "shift"), &[0.5], None, None);
    ctx.draw_elements(0x0004, 6, 0x1403, 0, 3);
    assert_eq!(ctx.get_error(), NO_ERROR);
    // Three instances shifted right by a quarter of the width each.
    for column in [1u32, 5, 9] {
        assert_eq!(
            pixel(&ctx, column, 14),
            [255, 255, 255, 255],
            "instance at column {column}"
        );
    }
    assert_eq!(pixel(&ctx, 13, 14), [0, 0, 0, 0]);
    // A fan with the same square.
    ctx.state.clear_color = [0.0; 4];
    ctx.clear(COLOR_BUFFER_BIT);
    ctx.draw_elements(0x0006, 4, 0x1403, 0, 1);
    assert_eq!(pixel(&ctx, 1, 14), [255, 255, 255, 255]);
}

#[test]
fn textures_are_sampled_and_a_framebuffer_renders_into_one() {
    let vs = "attribute vec2 position; varying vec2 uv; void main() { uv = position * 0.5 + 0.5; gl_Position = vec4(position, 0.0, 1.0); }";
    let fs = "precision mediump float; uniform sampler2D map; varying vec2 uv; void main() { gl_FragColor = texture2D(map, uv); }";
    let mut ctx = Context::new(8, 8);
    let p = build(&mut ctx, vs, fs);
    let b = ctx.create_buffer();
    ctx.bind_buffer(ARRAY_BUFFER, b);
    ctx.buffer_data(ARRAY_BUFFER, floats(&[-1.0, -1.0, 3.0, -1.0, -1.0, 3.0]));
    attrib(&mut ctx, p, "position", 2, 0, 0);
    // A 2x1 texture, left red, right blue.
    let t = ctx.create_texture();
    ctx.active_texture(0);
    ctx.bind_texture(TEXTURE_2D, t);
    ctx.tex_parameter(TEXTURE_2D, 0x2800, 0x2600);
    ctx.tex_parameter(TEXTURE_2D, 0x2801, 0x2600);
    ctx.tex_image_2d(
        TEXTURE_2D,
        0,
        0x1908,
        2,
        1,
        0x1908,
        0x1401,
        Some(&[255, 0, 0, 255, 0, 0, 255, 255]),
    );
    ctx.set_uniform(ctx.uniform_location(p, "map"), &[], Some(&[0]), None);
    ctx.draw_arrays(0x0004, 0, 3, 1);
    assert_eq!(ctx.get_error(), NO_ERROR);
    assert_eq!(pixel(&ctx, 1, 4), [255, 0, 0, 255]);
    assert_eq!(pixel(&ctx, 6, 4), [0, 0, 255, 255]);

    // Render a solid colour into a 4x4 texture through a framebuffer, then sample it.
    let target = ctx.create_texture();
    ctx.bind_texture(TEXTURE_2D, target);
    ctx.tex_image_2d(TEXTURE_2D, 0, 0x1908, 4, 4, 0x1908, 0x1401, None);
    ctx.tex_parameter(TEXTURE_2D, 0x2800, 0x2600);
    ctx.tex_parameter(TEXTURE_2D, 0x2801, 0x2600);
    let fbo = ctx.create_framebuffer();
    ctx.bind_framebuffer(FRAMEBUFFER, fbo);
    ctx.framebuffer_texture(COLOR_ATTACHMENT0, target, 0, 0);
    assert_eq!(ctx.check_framebuffer_status(), FRAMEBUFFER_COMPLETE);
    let solid = build(
        &mut ctx,
        "attribute vec2 position; void main() { gl_Position = vec4(position, 0.0, 1.0); }",
        "void main() { gl_FragColor = vec4(0.0, 1.0, 0.0, 1.0); }",
    );
    ctx.state.viewport = [0, 0, 4, 4];
    ctx.attrib_location(solid, "position");
    let loc = ctx.attrib_location(solid, "position") as u32;
    ctx.enable_vertex_attrib_array(loc, true);
    ctx.bind_buffer(ARRAY_BUFFER, b);
    ctx.vertex_attrib_pointer(loc, 2, 0x1406, false, 0, 0, false);
    ctx.draw_arrays(0x0004, 0, 3, 1);
    let px = ctx.read_pixels(1, 1, 1, 1);
    assert_eq!(px, vec![0, 255, 0, 255]);
    // Back on the screen, the texture is sampled green.
    ctx.bind_framebuffer(FRAMEBUFFER, 0);
    ctx.state.viewport = [0, 0, 8, 8];
    ctx.use_program(p);
    ctx.set_uniform(ctx.uniform_location(p, "map"), &[], Some(&[0]), None);
    ctx.active_texture(0);
    ctx.bind_texture(TEXTURE_2D, target);
    ctx.clear(COLOR_BUFFER_BIT);
    ctx.draw_arrays(0x0004, 0, 3, 1);
    assert_eq!(pixel(&ctx, 4, 4), [0, 255, 0, 255]);
}

#[test]
fn point_sprites_lines_scissor_and_discard() {
    let vs = "attribute vec2 position; void main() { gl_PointSize = 4.0; gl_Position = vec4(position, 0.0, 1.0); }";
    let fs = "void main() { if (gl_PointCoord.x > 0.9) discard; gl_FragColor = vec4(1.0); }";
    let mut ctx = Context::new(16, 16);
    let p = build(&mut ctx, vs, fs);
    let b = ctx.create_buffer();
    ctx.bind_buffer(ARRAY_BUFFER, b);
    ctx.buffer_data(ARRAY_BUFFER, floats(&[0.0, 0.0]));
    attrib(&mut ctx, p, "position", 2, 0, 0);
    ctx.draw_arrays(0x0000, 0, 1, 1);
    assert_eq!(
        pixel(&ctx, 7, 7),
        [255, 255, 255, 255],
        "a 4px point around the centre"
    );
    assert_eq!(pixel(&ctx, 6, 7), [255, 255, 255, 255]);
    assert_eq!(pixel(&ctx, 10, 7), [0, 0, 0, 0]);
    // Lines.
    ctx.clear(COLOR_BUFFER_BIT);
    ctx.buffer_data(ARRAY_BUFFER, floats(&[-1.0, 0.0, 1.0, 0.0]));
    ctx.vertex_attrib_pointer(0, 2, 0x1406, false, 0, 0, false);
    let fs_plain = build(
        &mut ctx,
        "attribute vec2 position; void main() { gl_Position = vec4(position, 0.0, 1.0); }",
        "void main() { gl_FragColor = vec4(1.0); }",
    );
    ctx.enable_vertex_attrib_array(ctx.attrib_location(fs_plain, "position") as u32, true);
    ctx.vertex_attrib_pointer(
        ctx.attrib_location(fs_plain, "position") as u32,
        2,
        0x1406,
        false,
        0,
        0,
        false,
    );
    ctx.draw_arrays(0x0001, 0, 2, 1);
    assert!(
        pixel(&ctx, 3, 7)[0] == 255 || pixel(&ctx, 3, 8)[0] == 255,
        "the line runs along the middle"
    );
    assert_eq!(pixel(&ctx, 3, 3)[0], 0);
    // Scissor confines drawing and clearing.
    ctx.state.scissor_test = true;
    ctx.state.scissor = [0, 0, 4, 4];
    ctx.state.clear_color = [1.0, 0.0, 0.0, 1.0];
    ctx.clear(COLOR_BUFFER_BIT);
    assert_eq!(pixel(&ctx, 1, 14), [255, 0, 0, 255]);
    assert_eq!(pixel(&ctx, 8, 14), [0, 0, 0, 0]);
}

#[test]
fn uniform_arrays_matrices_and_integer_attributes() {
    let vs = "attribute float id; uniform vec3 colors[3]; uniform mat2 m; varying vec3 c; void main() { c = colors[int(id)]; vec2 p = m * vec2(-1.0, -1.0); gl_Position = vec4(p, 0.0, 1.0); }";
    let fs = "varying vec3 c; void main() { gl_FragColor = vec4(c, 1.0); }";
    let mut ctx = Context::new(4, 4);
    let p = build(&mut ctx, vs, fs);
    assert_eq!(
        ctx.uniform_location(p, "colors[1]"),
        ctx.uniform_location(p, "colors") + 1
    );
    assert_eq!(ctx.uniform_location(p, "nothing"), -1);
    let infos = ctx.active_uniforms(p);
    assert!(infos.iter().any(|u| u.name == "colors[0]" && u.size == 3));
    ctx.set_uniform(
        ctx.uniform_location(p, "colors"),
        &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        None,
        None,
    );
    ctx.set_uniform(
        ctx.uniform_location(p, "m"),
        &[1.0, 0.0, 0.0, 1.0],
        None,
        Some((2, 2)),
    );
    let b = ctx.create_buffer();
    ctx.bind_buffer(ARRAY_BUFFER, b);
    ctx.buffer_data(ARRAY_BUFFER, floats(&[1.0, 1.0, 1.0]));
    attrib(&mut ctx, p, "id", 1, 0, 0);
    ctx.draw_arrays(0x0000, 0, 3, 1);
    assert_eq!(ctx.get_error(), NO_ERROR);
    // Three points at the same corner: the last (blue, id = 1 -> green) wins; all read colors[1].
    assert_eq!(ctx.read_pixels(0, 0, 1, 1), vec![0, 255, 0, 255]);
}

#[test]
fn a_program_that_does_not_link_reports_why() {
    let mut ctx = Context::new(2, 2);
    let (v, f) = (
        ctx.create_shader(VERTEX_SHADER),
        ctx.create_shader(FRAGMENT_SHADER),
    );
    ctx.shader_source(v, "void main() { gl_Position = vec4(0.0); }");
    ctx.shader_source(
        f,
        "varying vec2 missing; void main() { gl_FragColor = vec4(missing, 0.0, 1.0); }",
    );
    ctx.compile_shader(v);
    ctx.compile_shader(f);
    let p = ctx.create_program();
    ctx.attach_shader(p, v);
    ctx.attach_shader(p, f);
    ctx.link_program(p);
    assert!(!ctx.program_linked(p));
    assert!(ctx.program_log(p).contains("missing"));
    // A shader with a syntax error does not compile, and says where.
    let bad = ctx.create_shader(FRAGMENT_SHADER);
    ctx.shader_source(bad, "void main() { float x = ; }");
    ctx.compile_shader(bad);
    assert!(!ctx.shader_compiled(bad));
    assert!(ctx.shader_log(bad).contains("line 1"));
    // Drawing with an unlinked program is an error, not a panic.
    ctx.use_program(p);
    ctx.draw_arrays(0x0004, 0, 3, 1);
    assert_eq!(ctx.get_error(), INVALID_OPERATION);
}

#[test]
fn the_picture_is_unpremultiplied_top_down_rgba() {
    let mut ctx = Context::new(2, 2);
    ctx.state.clear_color = [0.5, 0.0, 0.0, 0.5];
    ctx.clear(COLOR_BUFFER_BIT);
    let pic = ctx.picture();
    assert_eq!(pic.len(), 16);
    assert_eq!(pic[3], 128);
    assert!(
        pic[0] >= 254,
        "premultiplied 0.5/0.5 is full red: {}",
        pic[0]
    );
    ctx.premultiplied_alpha = false;
    assert_eq!(ctx.picture()[0], 128);
}

#[test]
fn a_draw_that_runs_out_of_time_stops_and_a_context_that_keeps_doing_so_goes_quiet() {
    let fs = "precision highp float; void main() { float x = 0.0; for (int i = 0; i < 4000; i++) { x += sin(float(i)); } gl_FragColor = vec4(x * 0.0 + 1.0); }";
    let mut ctx = Context::new(256, 256);
    ctx.draw_budget = std::time::Duration::from_millis(20);
    let p = build(
        &mut ctx,
        "attribute vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }",
        fs,
    );
    let b = ctx.create_buffer();
    ctx.bind_buffer(ARRAY_BUFFER, b);
    ctx.buffer_data(ARRAY_BUFFER, floats(&[-1.0, -1.0, 3.0, -1.0, -1.0, 3.0]));
    attrib(&mut ctx, p, "p", 2, 0, 0);
    let start = std::time::Instant::now();
    for _ in 0..5 {
        ctx.draw_arrays(0x0004, 0, 3, 1);
    }
    assert!(
        start.elapsed() < std::time::Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
    assert!(ctx.slow_draws >= 3);
    // Not every pixel was shaded.
    let shaded = ctx.picture().chunks(4).filter(|p| p[3] != 0).count();
    assert!(shaded < 256 * 256);
}

#[test]
fn a_fragment_varying_the_vertex_shader_lacks_links_only_if_the_fragment_never_reads_it() {
    let vs = "void main() { gl_Position = vec4(0.0, 0.0, 0.0, 1.0); }";
    let mut ctx = Context::new(2, 2);
    let mut link = |fs: &str| {
        let (v, f) = (
            ctx.create_shader(VERTEX_SHADER),
            ctx.create_shader(FRAGMENT_SHADER),
        );
        ctx.shader_source(v, vs);
        ctx.shader_source(f, fs);
        ctx.compile_shader(v);
        ctx.compile_shader(f);
        assert!(ctx.shader_compiled(f), "{}", ctx.shader_log(f));
        let p = ctx.create_program();
        ctx.attach_shader(p, v);
        ctx.attach_shader(p, f);
        ctx.link_program(p);
        (ctx.program_linked(p), ctx.program_log(p))
    };
    // Declared and never read (a driver links this).
    let (ok, log) = link(
        "precision mediump float; varying vec3 vNormal; void main() { gl_FragColor = vec4(1.0); }",
    );
    assert!(ok, "{log}");
    // Read: the vertex shader has to provide it.
    let (ok, log) = link("precision mediump float; varying vec3 vNormal; void main() { gl_FragColor = vec4(vNormal, 1.0); }");
    assert!(!ok);
    assert!(log.contains("vNormal"), "{log}");
    // Read only inside a helper function, still a read.
    let (ok, _) = link("precision mediump float; varying vec3 vNormal; vec3 f() { return vNormal * 2.0; } void main() { gl_FragColor = vec4(f(), 1.0); }");
    assert!(!ok);
}
