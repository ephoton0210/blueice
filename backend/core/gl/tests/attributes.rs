// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! How vertex attributes are read: each case draws one point into a 1x1 target, with the
//! shader writing what it received into the colour, and reads the pixel back.

use blueice_gl::*;

const BYTE: u32 = 0x1401;
const SHORT: u32 = 0x1402;
const USHORT: u32 = 0x1403;
const INT: u32 = 0x1404;
const FLOAT: u32 = 0x1406;
const HALF: u32 = 0x140B;

fn setup(ty: &str, color: &str) -> (Context, u32) {
    let mut ctx = Context::new(1, 1);
    let vs = format!(
        "#version 300 es\nin {ty} a; out vec4 c; void main() {{ c = {color}; gl_Position = vec4(0.0, 0.0, 0.0, 1.0); gl_PointSize = 1.0; }}"
    );
    let fs =
        "#version 300 es\nprecision highp float; in vec4 c; out vec4 o; void main() { o = c; }";
    let (v, f) = (
        ctx.create_shader(VERTEX_SHADER),
        ctx.create_shader(FRAGMENT_SHADER),
    );
    ctx.shader_source(v, &vs);
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
    let location = ctx.attrib_location(p, "a");
    assert!(location >= 0);
    (ctx, location as u32)
}

fn data(ctx: &mut Context, bytes: Vec<u8>) {
    let b = ctx.create_buffer();
    ctx.bind_buffer(ARRAY_BUFFER, b);
    ctx.buffer_data(ARRAY_BUFFER, bytes);
}

fn pixel(ctx: &Context) -> [u8; 4] {
    let p = ctx.picture();
    [p[0], p[1], p[2], p[3]]
}

#[allow(clippy::too_many_arguments)]
fn pointer(
    ctx: &mut Context,
    loc: u32,
    size: u32,
    ty: u32,
    normalized: bool,
    stride: u32,
    offset: u32,
    integer: bool,
) {
    ctx.enable_vertex_attrib_array(loc, true);
    ctx.vertex_attrib_pointer(loc, size, ty, normalized, stride, offset, integer);
}

#[test]
fn normalized_unsigned_bytes_become_fractions() {
    let (mut ctx, loc) = setup("vec4", "a");
    data(&mut ctx, vec![255, 128, 0, 255]);
    pointer(&mut ctx, loc, 4, BYTE, true, 0, 0, false);
    ctx.draw_arrays(0x0000, 0, 1, 1);
    assert_eq!(pixel(&ctx), [255, 128, 0, 255]);
}

#[test]
fn unnormalized_bytes_keep_their_value() {
    let (mut ctx, loc) = setup("float", "vec4(a / 255.0, 0.0, 0.0, 1.0)");
    data(&mut ctx, vec![200]);
    pointer(&mut ctx, loc, 1, BYTE, false, 0, 0, false);
    ctx.draw_arrays(0x0000, 0, 1, 1);
    assert_eq!(pixel(&ctx), [200, 0, 0, 255]);
}

#[test]
fn normalized_shorts_and_a_stride_with_an_offset() {
    // Interleaved: a padding short, then the value, per vertex (stride 4, offset 2).
    let (mut ctx, loc) = setup("vec2", "vec4(a, 0.0, 1.0)");
    let mut bytes = Vec::new();
    for v in [1234i16, 32767, -32768, 777] {
        bytes.extend(v.to_le_bytes());
    }
    // Two vertices of (pad, x) - read the first one's value pair via stride 4 from offset 2.
    data(&mut ctx, bytes);
    pointer(&mut ctx, loc, 2, SHORT, true, 0, 4, false);
    ctx.draw_arrays(0x0000, 0, 1, 1);
    // Offset 4 bytes: (-32768 -> -1.0 clamps to 0), 777/32767 ~ 0.0237.
    let p = pixel(&ctx);
    assert_eq!(p[0], 0, "a negative fraction clamps to zero");
    assert_eq!(p[1], 6, "777/32767 of 255");
}

#[test]
fn unsigned_shorts_are_read_as_given() {
    let (mut ctx, loc) = setup("float", "vec4(a / 1000.0, 0.0, 0.0, 1.0)");
    data(&mut ctx, 500u16.to_le_bytes().to_vec());
    pointer(&mut ctx, loc, 1, USHORT, false, 0, 0, false);
    ctx.draw_arrays(0x0000, 0, 1, 1);
    assert_eq!(pixel(&ctx), [128, 0, 0, 255]);
}

#[test]
fn integer_attributes_arrive_as_integers() {
    let (mut ctx, loc) = setup("ivec2", "vec4(vec2(a) / 255.0, 0.0, 1.0)");
    let mut bytes = Vec::new();
    for v in [100i32, 200] {
        bytes.extend(v.to_le_bytes());
    }
    data(&mut ctx, bytes);
    pointer(&mut ctx, loc, 2, INT, false, 0, 0, true);
    ctx.draw_arrays(0x0000, 0, 1, 1);
    assert_eq!(pixel(&ctx), [100, 200, 0, 255]);
}

#[test]
fn a_disabled_attribute_supplies_its_constant() {
    let (mut ctx, loc) = setup("vec4", "a");
    ctx.vertex_attrib_constant(loc, [0.5, 0.25, 0.0, 1.0]);
    ctx.draw_arrays(0x0000, 0, 1, 1);
    assert_eq!(pixel(&ctx), [128, 64, 0, 255]);
}

#[test]
fn missing_components_default_to_zero_zero_zero_one() {
    let (mut ctx, loc) = setup("vec4", "a");
    let bytes: Vec<u8> = [0.25f32, 0.5]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    data(&mut ctx, bytes);
    pointer(&mut ctx, loc, 2, FLOAT, false, 0, 0, false);
    ctx.draw_arrays(0x0000, 0, 1, 1);
    assert_eq!(pixel(&ctx), [64, 128, 0, 255], "z is 0 and w is 1");
}

#[test]
fn a_divisor_advances_once_per_instance() {
    let (mut ctx, loc) = setup("float", "vec4(a / 255.0, 0.0, 0.0, 1.0)");
    data(
        &mut ctx,
        [10.0f32, 20.0, 30.0, 40.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect(),
    );
    pointer(&mut ctx, loc, 1, FLOAT, false, 0, 0, false);
    ctx.vertex_attrib_divisor(loc, 2);
    // Four instances of one vertex: instances 0-1 read the first element, 2-3 the second;
    // all land on the one pixel and the last wins.
    ctx.draw_arrays(0x0000, 0, 1, 4);
    assert_eq!(pixel(&ctx)[0], 20);
}

#[test]
fn half_floats_are_widened() {
    let (mut ctx, loc) = setup("float", "vec4(a, 0.0, 0.0, 1.0)");
    data(&mut ctx, 0x3800u16.to_le_bytes().to_vec()); // 0.5
    pointer(&mut ctx, loc, 1, HALF, false, 0, 0, false);
    ctx.draw_arrays(0x0000, 0, 1, 1);
    assert_eq!(pixel(&ctx), [128, 0, 0, 255]);
}

#[test]
fn a_matrix_attribute_reads_a_column_per_location() {
    let (mut ctx, loc) = setup("mat2", "vec4(a[0], a[1])");
    let bytes: Vec<u8> = [0.25f32, 0.5, 0.75, 1.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    data(&mut ctx, bytes);
    for column in 0..2u32 {
        pointer(
            &mut ctx,
            loc + column,
            2,
            FLOAT,
            false,
            16 / 2,
            column * 8,
            false,
        );
    }
    ctx.draw_arrays(0x0000, 0, 1, 1);
    assert_eq!(pixel(&ctx), [64, 128, 191, 255]);
}

#[test]
fn indexed_drawing_reads_each_index_and_repeats_agree() {
    // Three vertices, a triangle list that names vertex 1 twice; the shader writes its index.
    let mut ctx = Context::new(4, 1);
    let vs = "#version 300 es\nin float a; out float v; void main() { v = a; gl_Position = vec4(a * 2.0 - 1.0, 0.0, 0.0, 1.0); gl_PointSize = 1.0; }";
    let fs = "#version 300 es\nprecision highp float; in float v; out vec4 o; void main() { o = vec4(v, 0.0, 0.0, 1.0); }";
    let (sv, sf) = (
        ctx.create_shader(VERTEX_SHADER),
        ctx.create_shader(FRAGMENT_SHADER),
    );
    ctx.shader_source(sv, vs);
    ctx.shader_source(sf, fs);
    ctx.compile_shader(sv);
    ctx.compile_shader(sf);
    let p = ctx.create_program();
    ctx.attach_shader(p, sv);
    ctx.attach_shader(p, sf);
    ctx.link_program(p);
    assert!(ctx.program_linked(p), "{}", ctx.program_log(p));
    ctx.use_program(p);
    // Points at x = 0.125 and 0.875 of the width (pixels 0 and 3) after mapping a -> NDC.
    data(
        &mut ctx,
        [0.125f32, 0.375, 0.875]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect(),
    );
    let loc = ctx.attrib_location(p, "a") as u32;
    pointer(&mut ctx, loc, 1, FLOAT, false, 0, 0, false);
    let e = ctx.create_buffer();
    ctx.bind_buffer(ELEMENT_ARRAY_BUFFER, e);
    // Indices 0, 1, 1, 2, 0: a repeated index must give the same result as a fresh one.
    ctx.buffer_data(
        ELEMENT_ARRAY_BUFFER,
        [0u16, 1, 1, 2, 0]
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect(),
    );
    ctx.draw_elements(0x0000, 5, 0x1403, 0, 1);
    assert_eq!(ctx.get_error(), NO_ERROR);
    let pic = ctx.picture();
    let red = |x: usize| pic[x * 4];
    assert_eq!(red(0), 32, "vertex 0 (0.125)");
    assert_eq!(red(1), 96, "vertex 1 (0.375), drawn twice");
    assert_eq!(red(3), 223, "vertex 2 (0.875)");
}
