//! Tests for the M5.11 video effects: every effect runs, stays finite and in range, and is
//! deterministic; neutral parameters are the identity; each effect has a known-value check; the
//! frame/track environment works through the real compositor.

use std::sync::Arc;

use filmcraft_project::{EffectKind, Interpolation, Keyframe, Param, ParamKind, find_effect};

use super::*;

fn cx() -> FxCtx<'static> {
    FxCtx {
        t: Tick::ZERO,
        px_scale: 1.0,
        seconds: 0.0,
        timecode: "01:00:00:00",
        clip_name: "clip",
        project: None,
        env: None,
        working: filmcraft_color::WorkingSpace::Rec709,
    }
}

fn inst(id: &str) -> EffectInstance {
    find_effect(id).unwrap_or_else(|| panic!("no effect {id}")).instance()
}

fn set(e: &mut EffectInstance, k: &str, v: ParamValue) {
    e.params.get_mut(k).unwrap_or_else(|| panic!("{}: no param {k}", e.effect)).value = v;
}
fn setf(e: &mut EffectInstance, k: &str, v: f64) {
    set(e, k, ParamValue::Float(v));
}

/// A deterministic textured test picture (straight sRGB-ish values in linear premultiplied form).
fn picture(w: usize, h: usize) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            let n = noise3(x as f32 / 6.0, y as f32 / 6.0, 0.5, 9) * 0.25;
            let c = [(0.2 + 0.6 * u + n).clamp(0.0, 1.0), (0.3 + 0.5 * v - n).clamp(0.0, 1.0), (0.5 + 0.4 * (u - v) + n * 0.5).clamp(0.0, 1.0)];
            let l = dec(c);
            img.px[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[l[0], l[1], l[2], 1.0]);
        }
    }
    img
}

fn run(id: &str, e: &EffectInstance, img: &Image, cx: &FxCtx) -> Image {
    let mut o = img.clone();
    crate::effects::apply(&mut o, e, cx);
    let _ = id;
    o
}

fn max_diff(a: &Image, b: &Image) -> f32 {
    a.px.iter().zip(&b.px).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

/// The ids this milestone added or rebuilt.
const NEW_IDS: &[&str] = &[
    "lighting_effects",
    "bokeh_blur",
    "channel_blur",
    "compound_blur",
    "focus_blur",
    "reduce_interlace_flicker",
    "asc_cdl",
    "video_limiter",
    "vignette",
    "corner_pin",
    "magnify",
    "spherize",
    "turbulent_displace",
    "warp_stabilizer",
    "gradient",
    "channel_mix",
    "color_replace",
    "rounded_crop",
    "vr_blur",
    "vr_chromatic_aberrations",
    "vr_color_gradients",
    "vr_denoise",
    "vr_digital_glitch",
    "vr_fractal_noise",
    "vr_glow",
    "vr_plane_to_sphere",
    "vr_projection",
    "vr_rotate_sphere",
    "vr_sharpen",
    "alpha_adjust",
    "logo_cutout",
    "echo_glow",
    "edge_glow",
    "glint",
    "light_leaks",
    "rgb_split",
    "volumetric_rays",
    "wonder_glow",
    "long_shadow",
    "brush_strokes",
    "color_emboss",
    "roughen_edges",
    "posterize_time",
    "rotate_3d",
    "auto_reframe",
    "camera_shake",
    "grow",
    "shrink",
    "move",
    "spin",
    "spacer",
    "wiggle",
    "auto_align",
    "cineon_converter",
    "clone",
    "stroke",
    "metadata_burnin",
    "echo",
    "motion_extract",
    "lightning",
    "cell_pattern",
    "checkerboard",
    "ellipse",
    "paint_bucket",
    "write_on",
    "alpha_glow",
    "block_dissolve",
    "directional_blur_legacy",
    "gaussian_blur_legacy",
    "gradient_wipe_legacy",
    "linear_wipe_legacy",
    "magnify_legacy",
    "mosaic_legacy",
    "noise_legacy",
    "twirl_legacy",
    // rebuilt
    "ultra_key",
    "lens_flare",
    "track_matte",
    "simple_text",
    "noise",
    "mosaic",
];

#[test]
fn every_new_effect_is_defined_as_video() {
    for id in NEW_IDS {
        let d = find_effect(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(d.kind, EffectKind::Video, "{id}");
        assert!(!d.category.is_empty(), "{id}");
        for p in &d.params {
            if let ParamKind::Float { min, max, .. } = p.kind {
                let v = p.default.as_f64().unwrap();
                assert!(v >= min && v <= max, "{id}.{} default {v} outside {min}..{max}", p.id);
            }
        }
    }
}

/// Push every float parameter towards its soft maximum so the code paths run.
fn pushed(id: &str) -> EffectInstance {
    let def = find_effect(id).unwrap();
    let mut e = def.instance();
    for (pid, p) in e.params.iter_mut() {
        if let ParamValue::Float(v) = &mut p.value
            && let Some(ParamKind::Float { soft_max, soft_min, .. }) = def.param(pid).map(|d| &d.kind)
        {
            *v = (*v + (soft_max - soft_min) * 0.3).min(*soft_max);
        }
    }
    e
}

#[test]
fn every_effect_is_finite_in_range_and_deterministic() {
    let img = picture(40, 28);
    let c = FxCtx { seconds: 0.4, ..cx() };
    for id in NEW_IDS {
        for e in [inst(id), pushed(id)] {
            let a = run(id, &e, &img, &c);
            let b = run(id, &e, &img, &c);
            assert_eq!(a.px, b.px, "{id} not deterministic");
            assert_eq!((a.w, a.h), (img.w, img.h), "{id} changed size");
            for p in a.px.as_chunks::<4>().0 {
                assert!(p.iter().all(|v| v.is_finite()), "{id}: {p:?}");
                assert!(p[3] >= -1e-4 && p[3] <= 1.0 + 1e-3, "{id}: alpha {}", p[3]);
                assert!(p[..3].iter().all(|v| *v >= -1e-3), "{id}: negative {p:?}");
            }
        }
    }
}

#[test]
fn every_choice_value_runs() {
    let img = picture(24, 16);
    for id in NEW_IDS {
        let def = find_effect(id).unwrap();
        for p in &def.params {
            if let ParamKind::Choice(opts) = p.kind {
                for i in 0..opts.len() {
                    let mut e = pushed(id);
                    set(&mut e, p.id, ParamValue::Choice(i as u32));
                    let o = run(id, &e, &img, &cx());
                    assert!(o.px.iter().all(|v| v.is_finite()), "{id}.{}={i}", p.id);
                }
            }
        }
    }
}

#[test]
fn neutral_parameters_are_identity() {
    let img = picture(32, 24);
    let cases: Vec<(&str, Vec<(&str, ParamValue)>)> = vec![
        ("asc_cdl", vec![]),
        ("channel_mix", vec![]),
        ("vignette", vec![("amount", ParamValue::Float(0.0))]),
        ("corner_pin", vec![]),
        ("spherize", vec![]),
        ("turbulent_displace", vec![("amount", ParamValue::Float(0.0))]),
        ("rotate_3d", vec![]),
        ("grow", vec![]),
        ("spin", vec![]),
        ("alpha_adjust", vec![]),
        ("rounded_crop", vec![("radius", ParamValue::Float(0.0))]),
        ("block_dissolve", vec![]),
        ("gradient_wipe_legacy", vec![]),
        ("linear_wipe_legacy", vec![]),
        ("channel_blur", vec![]),
        ("bokeh_blur", vec![("amount", ParamValue::Float(0.0))]),
        ("reduce_interlace_flicker", vec![]),
        ("vr_blur", vec![]),
        ("vr_sharpen", vec![]),
        ("vr_denoise", vec![]),
        ("vr_digital_glitch", vec![]),
        ("vr_chromatic_aberrations", vec![]),
        ("vr_rotate_sphere", vec![]),
        ("vr_projection", vec![]),
        ("rgb_split", vec![("amount", ParamValue::Float(0.0))]),
        ("color_replace", vec![("target", ParamValue::Color([0.0, 0.0, 0.0, 1.0])), ("similarity", ParamValue::Float(0.0))]),
        ("clone", vec![("columns", ParamValue::Float(1.0))]),
        ("auto_align", vec![("horizontal", ParamValue::Choice(0)), ("vertical", ParamValue::Choice(0))]),
        ("auto_align", vec![]),
        ("magnify", vec![("magnification", ParamValue::Float(100.0))]),
        ("stroke", vec![("width", ParamValue::Float(0.0))]),
        ("long_shadow", vec![("length", ParamValue::Float(0.0))]),
        ("roughen_edges", vec![("border", ParamValue::Float(0.0))]),
        ("posterize_time", vec![]),
        ("echo", vec![]),
        ("warp_stabilizer", vec![]),
        ("track_matte", vec![]),
        ("noise", vec![]),
        ("noise_legacy", vec![]),
        ("gaussian_blur_legacy", vec![]),
        ("directional_blur_legacy", vec![]),
        ("video_limiter", vec![("compression", ParamValue::Choice(0))]),
        ("cineon_converter", vec![("conversion", ParamValue::Choice(2)), ("black10", ParamValue::Float(95.0)), ("white10", ParamValue::Float(685.0))]),
        ("light_leaks", vec![("intensity", ParamValue::Float(0.0))]),
        ("lens_flare", vec![("brightness", ParamValue::Float(0.0))]),
        ("simple_text", vec![("text", ParamValue::Text(String::new()))]),
    ];
    for (id, params) in cases {
        let mut e = inst(id);
        for (k, v) in params {
            set(&mut e, k, v);
        }
        let o = run(id, &e, &img, &cx());
        let d = max_diff(&o, &img);
        // in-range values pass the limiter's knee untouched; Cineon log→log at the default points
        assert!(d < 2e-3, "{id} not identity: max diff {d}");
    }
}

fn enc_px(img: &Image, x: usize, y: usize) -> [f32; 4] {
    let p = img.get(x, y);
    let c = enc(Image::unpremul(p));
    [c[0], c[1], c[2], p[3]]
}
fn solid(w: usize, h: usize, c: [f32; 3]) -> Image {
    let l = dec(c);
    Image::filled(w, h, [l[0], l[1], l[2], 1.0])
}
fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn asc_cdl_known_values() {
    // slope 2, offset 0.1, power 2 on 0.2 → (0.5)² = 0.25; saturation 0 → luma
    let out = color::cdl([0.2, 0.2, 0.2], [2.0; 3], [0.1; 3], [2.0; 3], 1.0);
    assert!(out.iter().all(|v| close(*v, 0.25, 1e-6)), "{out:?}");
    let grey = color::cdl([1.0, 0.0, 0.0], [1.0; 3], [0.0; 3], [1.0; 3], 0.0);
    assert!(grey.iter().all(|v| close(*v, 0.2126, 1e-5)), "{grey:?}");
    let mut e = inst("asc_cdl");
    setf(&mut e, "r_slope", 2.0);
    let o = run("asc_cdl", &e, &solid(4, 4, [0.25, 0.25, 0.25]), &cx());
    let p = enc_px(&o, 1, 1);
    assert!(close(p[0], 0.5, 2e-3) && close(p[1], 0.25, 2e-3), "{p:?}");
}

#[test]
fn channel_mix_swaps_channels() {
    let mut e = inst("channel_mix");
    for (k, v) in [("rr", 0.0), ("rb", 100.0), ("bb", 0.0), ("br", 100.0)] {
        setf(&mut e, k, v);
    }
    let o = run("channel_mix", &e, &solid(4, 4, [0.8, 0.4, 0.1]), &cx());
    let p = enc_px(&o, 0, 0);
    assert!(close(p[0], 0.1, 2e-3) && close(p[1], 0.4, 2e-3) && close(p[2], 0.8, 2e-3), "{p:?}");
}

#[test]
fn video_limiter_clips_super_whites() {
    let img = Image::filled(4, 4, [3.0, 3.0, 3.0, 1.0]);
    let o = run("video_limiter", &inst("video_limiter"), &img, &cx());
    let p = o.get(0, 0);
    assert!(p[0] <= 1.0 + 1e-4 && p[0] > 0.9, "{p:?}");
    // 105 IRE allows a little more
    let mut e = inst("video_limiter");
    set(&mut e, "clip_level", ParamValue::Choice(5));
    set(&mut e, "compression", ParamValue::Choice(0));
    let o = run("video_limiter", &e, &img, &cx());
    assert!(o.get(0, 0)[0] > 1.0, "{:?}", o.get(0, 0));
    // gamut warning paints the clipped pixels
    let mut e = inst("video_limiter");
    set(&mut e, "gamut_warning", ParamValue::Bool(true));
    let o = run("video_limiter", &e, &img, &cx());
    let p = o.get(0, 0);
    assert!(p[0] > 0.9 && p[1] < 0.01, "{p:?}");
    // the limiter keeps legal R′G′B′ and the luma knee
    let v = color::limit([1.2, 0.2, -0.1], 1.0, 0.0, 2);
    assert!(v.iter().all(|c| *c >= 0.0 && *c <= 1.0), "{v:?}");
}

#[test]
fn vignette_darkens_corners_only() {
    let img = solid(64, 36, [0.6, 0.6, 0.6]);
    let o = run("vignette", &inst("vignette"), &img, &cx());
    assert!(enc_px(&o, 0, 0)[0] < 0.45, "corner {:?}", enc_px(&o, 0, 0));
    assert!(close(enc_px(&o, 32, 18)[0], 0.6, 2e-3), "centre {:?}", enc_px(&o, 32, 18));
}

#[test]
fn corner_pin_maps_corners() {
    let img = picture(40, 30);
    let mut e = inst("corner_pin");
    // shrink the right half to the middle: UR (40,0)→(20,0), LR (40,30)→(20,30)
    set(&mut e, "upper_left", ParamValue::Vec2(Vec2::new(0.0, 0.0)));
    set(&mut e, "lower_left", ParamValue::Vec2(Vec2::new(0.0, 30.0)));
    set(&mut e, "upper_right", ParamValue::Vec2(Vec2::new(20.0, 0.0)));
    set(&mut e, "lower_right", ParamValue::Vec2(Vec2::new(20.0, 30.0)));
    let o = run("corner_pin", &e, &img, &cx());
    assert_eq!(o.get(30, 15)[3], 0.0, "outside the pinned quad is clear");
    // output x maps to source 2x
    let a = o.get(5, 10);
    let b = img.sample_bilinear(11.0, 10.5);
    for k in 0..4 {
        assert!(close(a[k], b[k], 1e-3), "{a:?} {b:?}");
    }
    // the homography helpers invert exactly
    let q = [Vec2::new(3.0, 1.0), Vec2::new(30.0, 5.0), Vec2::new(28.0, 25.0), Vec2::new(1.0, 20.0)];
    let h = distort::square_to_quad(q);
    for (i, (u, v)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].iter().enumerate() {
        let (x, y) = distort::h_apply(&h, *u, *v).unwrap();
        assert!((x - q[i].x).abs() < 1e-9 && (y - q[i].y).abs() < 1e-9);
    }
}

#[test]
fn magnify_and_spherize_enlarge_the_centre() {
    let img = picture(60, 60);
    let o = run("magnify", &inst("magnify"), &img, &cx());
    // 200 %: the pixel 10 px right of centre shows the source 5 px right of centre
    let a = o.get(40, 30);
    let b = img.sample_bilinear(35.25, 30.5);
    for k in 0..3 {
        assert!(close(a[k], b[k], 0.02), "{a:?} {b:?}");
    }
    let mut e = inst("spherize");
    setf(&mut e, "radius", 25.0);
    let o = run("spherize", &e, &img, &cx());
    assert!(max_diff(&o, &img) > 0.01);
    // outside the radius nothing moves
    assert_eq!(o.get(1, 1), img.get(1, 1));
}

#[test]
fn turbulent_displace_moves_pixels_and_pins_edges() {
    let img = picture(48, 48);
    let o = run("turbulent_displace", &inst("turbulent_displace"), &img, &cx());
    assert!(max_diff(&o, &img) > 0.01);
    // Pin All: the corners stay
    for (x, y) in [(0, 0), (47, 0), (0, 47), (47, 47)] {
        let (a, b) = (o.get(x, y), img.get(x, y));
        assert!((0..4).all(|k| close(a[k], b[k], 0.03)), "{x},{y}: {a:?} {b:?}");
    }
    // evolution changes the pattern; seed too
    let mut e = inst("turbulent_displace");
    setf(&mut e, "evolution", 90.0);
    assert!(max_diff(&run("t", &e, &img, &cx()), &o) > 1e-3);
}

#[test]
fn rotate_3d_z_matches_a_2d_rotation() {
    let img = picture(41, 41);
    let mut e = inst("rotate_3d");
    setf(&mut e, "rot_z", 90.0);
    let o = run("rotate_3d", &e, &img, &cx());
    // rotating 90° clockwise about the centre: output (x,y) samples source (y, 41−x)
    let a = o.get(30, 10);
    let b = img.sample_bilinear(10.5, 41.0 - 30.5);
    for k in 0..3 {
        assert!(close(a[k], b[k], 0.02), "{a:?} {b:?}");
    }
    // a Y rotation past 90° shows the back face; Hide Back Face clears it
    setf(&mut e, "rot_z", 0.0);
    setf(&mut e, "rot_y", 180.0);
    set(&mut e, "hide_back", ParamValue::Bool(true));
    assert!(run("r", &e, &img, &cx()).px.iter().all(|v| *v == 0.0));
}

#[test]
fn track_matte_uses_the_other_tracks_alpha_and_luma() {
    struct Env(Image);
    impl FxEnv for Env {
        fn frame(&self, _: f64, _: &EffectInstance) -> Option<Image> {
            None
        }
        fn source_frame(&self, _: f64, _: f32) -> Option<Image> {
            None
        }
        fn track(&self, i: usize) -> Option<Image> {
            (i == 1).then(|| self.0.clone())
        }
        fn layer_to_output(&self) -> Affine {
            Affine::IDENTITY
        }
        fn clip_seconds(&self) -> f64 {
            1.0
        }
        fn clip_offset(&self) -> f64 {
            0.0
        }
        fn frame_rate(&self) -> f64 {
            24.0
        }
        fn media_timecode(&self) -> String {
            String::new()
        }
        fn file_name(&self) -> String {
            String::new()
        }
        fn sequence_name(&self) -> String {
            String::new()
        }
        fn sequence_size(&self) -> (u32, u32) {
            (8, 4)
        }
        fn source_size(&self) -> (u32, u32) {
            (8, 4)
        }
        fn clip_key(&self) -> u64 {
            1
        }
    }
    // matte: left half opaque white, right half transparent
    let mut matte = Image::new(8, 4);
    for y in 0..4 {
        for x in 0..4 {
            matte.px[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
        }
    }
    let env = Env(matte);
    let c = FxCtx { env: Some(&env), ..cx() };
    let img = solid(8, 4, [0.5, 0.5, 0.5]);
    let mut e = inst("track_matte");
    set(&mut e, "matte", ParamValue::Choice(2));
    let o = run("track_matte", &e, &img, &c);
    assert!(close(o.get(1, 1)[3], 1.0, 1e-4) && close(o.get(6, 1)[3], 0.0, 1e-4));
    set(&mut e, "reverse", ParamValue::Bool(true));
    let o = run("track_matte", &e, &img, &c);
    assert!(close(o.get(1, 1)[3], 0.0, 1e-4) && close(o.get(6, 1)[3], 1.0, 1e-4));
    set(&mut e, "reverse", ParamValue::Bool(false));
    set(&mut e, "composite", ParamValue::Choice(1));
    let o = run("track_matte", &e, &img, &c);
    assert!(close(o.get(1, 1)[3], 1.0, 1e-3) && close(o.get(6, 1)[3], 0.0, 1e-4));
}

#[test]
fn keyers_known_values() {
    // Ultra Key: pure green screen → transparent, grey foreground → opaque
    let mut img = solid(8, 2, [0.0, 0.8, 0.2]);
    let grey = dec([0.5, 0.45, 0.42]);
    for x in 4..8 {
        for y in 0..2 {
            img.px[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].copy_from_slice(&[grey[0], grey[1], grey[2], 1.0]);
        }
    }
    let o = run("ultra_key", &inst("ultra_key"), &img, &cx());
    assert!(o.get(1, 0)[3] < 0.05, "screen {:?}", o.get(1, 0));
    assert!(o.get(6, 0)[3] > 0.95, "fg {:?}", o.get(6, 0));
    let mut e = inst("ultra_key");
    set(&mut e, "output", ParamValue::Choice(1));
    let m = run("ultra_key", &e, &img, &cx());
    assert!(m.get(1, 0)[0] < 0.05 && m.get(6, 0)[0] > 0.9 && m.get(1, 0)[3] == 1.0);
    let mut aggressive = inst("ultra_key");
    set(&mut aggressive, "setting", ParamValue::Choice(2));
    let ag = run("ultra_key", &aggressive, &img, &cx());
    let def = run("ultra_key", &inst("ultra_key"), &img, &cx());
    let delta = (0..img.w * img.h).map(|i| (ag.px[i * 4 + 3] - def.px[i * 4 + 3]).abs()).fold(0.0f32, f32::max);
    assert!(delta > 1e-4, "Aggressive Setting left the Default matte unchanged");
    // Alpha Adjust: invert and opacity
    let mut a = Image::filled(2, 2, [0.1, 0.1, 0.1, 0.25]);
    let mut e = inst("alpha_adjust");
    set(&mut e, "invert", ParamValue::Bool(true));
    setf(&mut e, "opacity", 50.0);
    crate::effects::apply(&mut a, &e, &cx());
    assert!(close(a.get(0, 0)[3], 0.375, 1e-5), "{:?}", a.get(0, 0));
    // Logo Cutout: the white background goes, a black logo stays black and opaque
    let mut img = solid(4, 1, [1.0, 1.0, 1.0]);
    img.px[0..4].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
    let o = run("logo_cutout", &inst("logo_cutout"), &img, &cx());
    assert_eq!(o.get(2, 0)[3], 0.0);
    assert!(close(o.get(0, 0)[3], 1.0, 1e-4) && o.get(0, 0)[0] < 1e-4);
    // 50 % grey over white un-multiplies to black at alpha 0.5
    let mut e = inst("logo_cutout");
    setf(&mut e, "threshold", 0.0);
    setf(&mut e, "softness", 100.0);
    let o = run("logo_cutout", &e, &solid(1, 1, [0.5, 0.5, 0.5]), &cx());
    let p = o.get(0, 0);
    assert!(close(p[3], 0.5, 1e-3) && p[0] < 1e-3, "{p:?}");
}

#[test]
fn colour_replace_and_cineon() {
    let o = run("color_replace", &inst("color_replace"), &solid(2, 2, [1.0, 0.0, 0.0]), &cx());
    let p = enc_px(&o, 0, 0);
    assert!(p[2] > 0.9 && p[0] < 0.1, "{p:?}");
    // Cineon: code 685 → white, 95 → black; log→lin→log round trip
    let (b, w) = (95.0, 685.0);
    assert!(close(color::cineon_value(685.0 / 1023.0, 0, b, w, 0.0, 1.0, 1.7, 0.0), 1.0, 1e-4));
    assert!(close(color::cineon_value(95.0 / 1023.0, 0, b, w, 0.0, 1.0, 1.7, 0.0), 0.0, 1e-4));
    for v in [0.2f32, 0.4, 0.6] {
        let l = color::cineon_value(v, 0, b, w, 0.0, 1.0, 1.7, 0.0);
        let back = color::cineon_value(l, 1, b, w, 0.0, 1.0, 1.7, 0.0);
        assert!(close(back, v, 2e-3), "{v} → {l} → {back}");
    }
}

#[test]
fn lights_add_light_around_highlights() {
    let mut img = solid(48, 48, [0.05, 0.05, 0.05]);
    for y in 22..26 {
        for x in 22..26 {
            img.px[(y * 48 + x) * 4..(y * 48 + x) * 4 + 4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
        }
    }
    let total = |i: &Image| i.px.as_chunks::<4>().0.iter().map(|p| p[0] as f64).sum::<f64>();
    for id in ["echo_glow", "glint", "wonder_glow", "volumetric_rays", "edge_glow"] {
        let o = run(id, &inst(id), &img, &cx());
        let added = total(&o) - total(&img);
        assert!(added > 0.5, "{id}: added only {added}");
        assert!(o.get(2, 2)[0] < 0.2, "{id}: lit the far corner {:?}", o.get(2, 2));
    }
    // Glint with 4 rays at 0°: light along the horizontal, not the diagonal
    let mut e = inst("glint");
    setf(&mut e, "rotation", 0.0);
    let o = run("glint", &e, &img, &cx());
    assert!(o.get(38, 24)[0] > o.get(34, 34)[0], "{:?} {:?}", o.get(38, 24), o.get(34, 34));
    // RGB Split: red sampled from the left
    let mut s = Image::new(9, 1);
    s.px[4 * 4..4 * 4 + 4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
    let mut e = inst("rgb_split");
    setf(&mut e, "amount", 2.0);
    let o = run("rgb_split", &e, &s, &cx());
    assert!(close(o.get(6, 0)[0], 1.0, 1e-4) && close(o.get(2, 0)[2], 1.0, 1e-4) && close(o.get(4, 0)[1], 1.0, 1e-4));
}

#[test]
fn long_shadow_and_stroke_geometry() {
    // a 4×4 opaque square at (4,4) on a transparent 24×24 layer
    let mut img = Image::new(24, 24);
    for y in 4..8 {
        for x in 4..8 {
            img.px[(y * 24 + x) * 4..(y * 24 + x) * 4 + 4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
        }
    }
    let mut e = inst("long_shadow");
    setf(&mut e, "angle", 90.0); // to the right
    setf(&mut e, "length", 10.0);
    set(&mut e, "fade", ParamValue::Bool(false));
    setf(&mut e, "opacity", 100.0);
    let o = run("long_shadow", &e, &img, &cx());
    assert!(o.get(14, 5)[3] > 0.8, "shadow to the right {:?}", o.get(14, 5));
    assert!(o.get(20, 5)[3] < 0.2, "ends after Length {:?}", o.get(20, 5));
    assert!(o.get(5, 14)[3] < 0.05, "not below {:?}", o.get(5, 14));
    // Stroke outside 2 px
    let mut e = inst("stroke");
    setf(&mut e, "width", 2.0);
    set(&mut e, "color", ParamValue::Color([1.0, 0.0, 0.0, 1.0]));
    let o = run("stroke", &e, &img, &cx());
    assert!(o.get(9, 5)[0] > 0.9 && o.get(9, 5)[1] < 0.05, "stroke {:?}", o.get(9, 5));
    assert!(o.get(12, 5)[3] < 0.01, "beyond the stroke {:?}", o.get(12, 5));
    assert_eq!(o.get(5, 5), [1.0, 1.0, 1.0, 1.0], "inside untouched");
    // exact distance transform
    let mut inside = vec![false; 7 * 5];
    inside[2 * 7 + 3] = true;
    let d = stylize::distance_to(&inside, 7, 5);
    assert!(close(d[2 * 7 + 3], 0.0, 1e-6) && close(d[0], (9.0f32 + 4.0).sqrt(), 1e-5));
}

#[test]
fn generators_known_values() {
    let img = solid(40, 20, [0.5, 0.5, 0.5]);
    // Gradient: black at the top centre, white at the bottom centre
    let mut e = inst("gradient");
    set(&mut e, "start", ParamValue::Vec2(Vec2::new(20.0, 0.0)));
    set(&mut e, "end", ParamValue::Vec2(Vec2::new(20.0, 20.0)));
    let o = run("gradient", &e, &img, &cx());
    assert!(enc_px(&o, 20, 0)[0] < 0.05 && enc_px(&o, 20, 19)[0] > 0.95 && close(enc_px(&o, 20, 10)[0], 0.525, 0.03));
    // Checkerboard: alternating cells
    let mut e = inst("checkerboard");
    setf(&mut e, "width", 10.0);
    set(&mut e, "anchor", ParamValue::Vec2(Vec2::new(0.0, 0.0)));
    let o = run("checkerboard", &e, &img, &cx());
    assert!(o.get(5, 5)[3] < 0.01 && o.get(15, 5)[3] > 0.99 && o.get(15, 15)[3] < 0.01);
    // Ellipse ring: on the ring opaque, at the centre clear
    let mut e = inst("ellipse");
    setf(&mut e, "width", 16.0);
    setf(&mut e, "height", 16.0);
    setf(&mut e, "thickness", 2.0);
    let o = run("ellipse", &e, &img, &cx());
    assert!(o.get(28, 10)[3] > 0.8 && o.get(20, 10)[3] < 0.01, "{:?} {:?}", o.get(28, 10), o.get(20, 10));
    // Cell Pattern is opaque grey in 0..1
    let o = run("cell_pattern", &inst("cell_pattern"), &img, &cx());
    assert!(o.px.as_chunks::<4>().0.iter().all(|p| p[3] == 1.0 && p[0] >= 0.0 && p[0] <= 1.0 && p[0] == p[1]));
    // Lightning: deterministic per seed, different across seeds and strikes
    let a = run("lightning", &inst("lightning"), &img, &cx());
    let mut e = inst("lightning");
    setf(&mut e, "seed", 5.0);
    assert!(max_diff(&a, &run("lightning", &e, &img, &cx())) > 0.01);
    let later = FxCtx { seconds: 2.0, ..cx() };
    assert!(max_diff(&a, &run("lightning", &inst("lightning"), &img, &later)) > 0.01);
}

#[test]
fn paint_bucket_fills_the_connected_region() {
    // left half red, right half blue; fill at the left
    let mut img = solid(10, 4, [1.0, 0.0, 0.0]);
    let blue = dec([0.0, 0.0, 1.0]);
    for y in 0..4 {
        for x in 5..10 {
            img.px[(y * 10 + x) * 4..(y * 10 + x) * 4 + 4].copy_from_slice(&[blue[0], blue[1], blue[2], 1.0]);
        }
    }
    let mut e = inst("paint_bucket");
    set(&mut e, "point", ParamValue::Vec2(Vec2::new(1.0, 1.0)));
    set(&mut e, "color", ParamValue::Color([0.0, 1.0, 0.0, 1.0]));
    let o = run("paint_bucket", &e, &img, &cx());
    assert!(enc_px(&o, 1, 1)[1] > 0.95 && enc_px(&o, 1, 1)[0] < 0.05);
    assert!(enc_px(&o, 8, 1)[2] > 0.95 && enc_px(&o, 8, 1)[1] < 0.05);
}

#[test]
fn write_on_follows_the_animated_brush() {
    use filmcraft_project::{Interpolation, Keyframe};
    let img = solid(40, 20, [0.0, 0.0, 0.0]);
    let mut e = inst("write_on");
    let p = e.params.get_mut("brush").unwrap();
    p.keyframes = vec![
        Keyframe {
            time: Tick::ZERO,
            value: ParamValue::Vec2(Vec2::new(5.0, 10.0)),
            interp: Interpolation::Linear,
            out_influence: 1.0 / 3.0,
            in_influence: 1.0 / 3.0,
        },
        Keyframe {
            time: Tick::from_seconds_f64(1.0),
            value: ParamValue::Vec2(Vec2::new(35.0, 10.0)),
            interp: Interpolation::Linear,
            out_influence: 1.0 / 3.0,
            in_influence: 1.0 / 3.0,
        },
    ];
    // half way: painted from x=5 to x=20, not beyond
    let c = FxCtx { t: Tick::from_seconds_f64(0.5), seconds: 0.5, ..cx() };
    let o = run("write_on", &e, &img, &c);
    assert!(enc_px(&o, 12, 10)[0] > 0.9, "{:?}", enc_px(&o, 12, 10));
    assert!(enc_px(&o, 30, 10)[0] < 0.05, "{:?}", enc_px(&o, 30, 10));
}

#[test]
fn transform_presets_animate_over_the_clip() {
    struct Env;
    impl FxEnv for Env {
        fn frame(&self, _: f64, _: &EffectInstance) -> Option<Image> {
            None
        }
        fn source_frame(&self, _: f64, _: f32) -> Option<Image> {
            None
        }
        fn track(&self, _: usize) -> Option<Image> {
            None
        }
        fn layer_to_output(&self) -> Affine {
            Affine::IDENTITY
        }
        fn clip_seconds(&self) -> f64 {
            2.0
        }
        fn clip_offset(&self) -> f64 {
            0.0
        }
        fn frame_rate(&self) -> f64 {
            24.0
        }
        fn media_timecode(&self) -> String {
            "00:00:10:00".into()
        }
        fn file_name(&self) -> String {
            "A001.mov".into()
        }
        fn sequence_name(&self) -> String {
            "Seq".into()
        }
        fn sequence_size(&self) -> (u32, u32) {
            (40, 40)
        }
        fn source_size(&self) -> (u32, u32) {
            (40, 40)
        }
        fn clip_key(&self) -> u64 {
            2
        }
    }
    let env = Env;
    let img = picture(40, 40);
    let end = FxCtx { seconds: 2.0, env: Some(&env), ..cx() };
    // Grow ends at 120 %
    let o = run("grow", &inst("grow"), &img, &end);
    let a = o.get(30, 20);
    let b = img.sample_bilinear(20.0 + 10.5 / 1.2, 20.0 + 0.5 / 1.2);
    assert!((0..3).all(|k| close(a[k], b[k], 0.02)), "{a:?} {b:?}");
    // Spin ends at 360° (identity again); half way it is upside down
    let o = run("spin", &inst("spin"), &img, &end);
    assert!(max_diff(&o, &img) < 0.02);
    let mid = FxCtx { seconds: 1.0, env: Some(&env), ..cx() };
    let o = run("spin", &inst("spin"), &img, &mid);
    let (a, b) = (o.get(10, 10), img.get(29, 29));
    assert!((0..3).all(|k| close(a[k], b[k], 0.03)), "{a:?} {b:?}");
    // Move from (−100, 0) to (0, 0): at the end identity, at the start shifted out
    assert!(max_diff(&run("move", &inst("move"), &img, &end), &img) < 1e-4);
    // burn-in text sources
    let e = inst("metadata_burnin");
    assert_eq!(text::burnin_text(&e, &end), "01:00:00:00");
    let mut e2 = e.clone();
    set(&mut e2, "source", ParamValue::Choice(1));
    assert_eq!(text::burnin_text(&e2, &end), "00:00:10:00");
    set(&mut e2, "source", ParamValue::Choice(3));
    set(&mut e2, "prefix", ParamValue::Text("FILE".into()));
    assert_eq!(text::burnin_text(&e2, &end), "FILE A001.mov");
    set(&mut e2, "source", ParamValue::Choice(4));
    assert_eq!(text::burnin_text(&e2, &mid), "FILE 24");
}

#[test]
fn transform_shutter_override_blurs_moving_position() {
    let mut e = inst("transform");
    set(&mut e, "shutter_override", ParamValue::Bool(true));
    setf(&mut e, "shutter_angle", 180.0);
    let a = Keyframe::new(Tick::ZERO, ParamValue::Vec2(filmcraft_geom::Vec2::new(8.0, 16.0)));
    let mut b = Keyframe::new(Tick::from_seconds_f64(1.0), ParamValue::Vec2(filmcraft_geom::Vec2::new(40.0, 16.0)));
    b.interp = Interpolation::Linear;
    e.params.insert("position".into(), Param { value: ParamValue::Vec2(filmcraft_geom::Vec2::new(8.0, 16.0)), keyframes: vec![a, b] });
    let img = picture(32, 32);
    let cx = FxCtx { t: Tick::from_seconds_f64(0.5), seconds: 0.5, ..cx() };
    let sharp = {
        let mut off = e.clone();
        set(&mut off, "shutter_override", ParamValue::Bool(false));
        run("transform", &off, &img, &cx)
    };
    let blurred = run("transform", &e, &img, &cx);
    let delta = max_diff(&sharp, &blurred);
    assert!(delta > 1e-3, "shutter override left the Transform picture unchanged ({delta})");
}

#[test]
fn clone_auto_align_and_rounded_crop() {
    let img = picture(40, 20);
    let mut e = inst("clone");
    setf(&mut e, "columns", 2.0);
    let o = run("clone", &e, &img, &cx());
    // two half-size copies side by side, letterboxed vertically
    assert_eq!(o.get(10, 1)[3], 0.0);
    let (a, b) = (o.get(10, 10), o.get(30, 10));
    assert!((0..4).all(|k| close(a[k], b[k], 1e-5)));
    // Auto Align left: content moves to x = 0
    let mut layer = Image::new(20, 10);
    for y in 3..6 {
        for x in 10..14 {
            layer.px[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
        }
    }
    let mut e = inst("auto_align");
    set(&mut e, "horizontal", ParamValue::Choice(1));
    set(&mut e, "vertical", ParamValue::Choice(1));
    let o = run("auto_align", &e, &layer, &cx());
    assert_eq!(o.get(0, 0)[3], 1.0);
    assert_eq!(o.get(4, 0)[3], 0.0);
    // Rounded Crop: corners are cut, the centre stays
    let o = run("rounded_crop", &inst("rounded_crop"), &picture(100, 100), &cx());
    assert_eq!(o.get(0, 0)[3], 0.0);
    assert_eq!(o.get(50, 50)[3], 1.0);
}

#[test]
fn vr_rotate_sphere_pans_exactly() {
    let img = picture(64, 32);
    let mut e = inst("vr_rotate_sphere");
    setf(&mut e, "pan", 360.0);
    assert!(max_diff(&run("vr", &e, &img, &cx()), &img) < 1e-4);
    setf(&mut e, "pan", 90.0);
    let o = run("vr", &e, &img, &cx());
    // a quarter turn is a 16-pixel horizontal shift (with wrap)
    let mut moved = 0.0f32;
    for y in 4..28 {
        for x in 0..64 {
            let a = o.get(x, y);
            let b = img.get((x + 64 - 16) % 64, y);
            let c = img.get((x + 16) % 64, y);
            moved = moved.max((0..3).map(|k| (a[k] - b[k]).abs()).fold(0.0, f32::max).min((0..3).map(|k| (a[k] - c[k]).abs()).fold(0.0, f32::max)));
        }
    }
    assert!(moved < 2e-3, "{moved}");
    // VR Blur is seamless across the ±180° edge
    let mut e = inst("vr_blur");
    setf(&mut e, "blurriness", 8.0);
    let o = run("vr_blur", &e, &img, &cx());
    let seam = |i: &Image| (0..3).map(|k| (i.get(0, 16)[k] - i.get(63, 16)[k]).abs()).fold(0.0, f32::max);
    assert!(seam(&o) < seam(&img) * 0.3, "{} vs {}", seam(&o), seam(&img));
    // Plane to Sphere: the flat picture lands in front (lon 0), nothing behind
    let o = run("vr_plane_to_sphere", &inst("vr_plane_to_sphere"), &img, &cx());
    assert!(o.get(32, 16)[3] > 0.99 && o.get(0, 16)[3] == 0.0);
    // stereoscopic layouts process each half
    let mut e = inst("vr_rotate_sphere");
    setf(&mut e, "pan", 90.0);
    set(&mut e, "frame_layout", ParamValue::Choice(1));
    let o = run("vr", &e, &img, &cx());
    assert_eq!((o.w, o.h), (64, 32));
}

#[test]
fn bokeh_and_focus_blur() {
    let mut img = solid(48, 48, [0.0, 0.0, 0.0]);
    img.px[(24 * 48 + 24) * 4..(24 * 48 + 24) * 4 + 4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
    let mut e = inst("bokeh_blur");
    setf(&mut e, "amount", 8.0);
    set(&mut e, "shape", ParamValue::Choice(2)); // square
    setf(&mut e, "rotation", 45.0); // axis-aligned, half-side 8/√2
    let o = run("bokeh", &e, &img, &cx());
    // the bright point becomes a square: lit at (+5, +5), dark at (+7, 0) where a disc would be lit
    assert!(o.get(29, 29)[0] > 1e-4 && o.get(31, 24)[0] < 1e-6, "{:?} {:?}", o.get(29, 29), o.get(31, 24));
    set(&mut e, "shape", ParamValue::Choice(0));
    let o = run("bokeh", &e, &img, &cx());
    assert!(o.get(31, 24)[0] > 1e-4);
    // Focus Blur: the focus point stays sharp
    let pic = picture(64, 64);
    let mut e = inst("focus_blur");
    setf(&mut e, "size", 20.0);
    setf(&mut e, "feather", 10.0);
    setf(&mut e, "amount", 20.0);
    let o = run("focus", &e, &pic, &cx());
    assert!(max_diff(&o, &pic) > 0.01);
    let (a, b) = (o.get(32, 32), pic.get(32, 32));
    assert!((0..4).all(|k| close(a[k], b[k], 1e-4)));
}

#[test]
fn echo_combines_operators() {
    let a = Image::filled(1, 1, [0.2, 0.4, 0.1, 1.0]);
    let b = Image::filled(1, 1, [0.6, 0.1, 0.3, 1.0]);
    let layers = vec![(a.clone(), 1.0), (b.clone(), 0.5)];
    let add = temporal::combine_echoes(&layers, 0);
    assert!(close(add.get(0, 0)[0], 0.5, 1e-6) && close(add.get(0, 0)[3], 1.0, 1e-6));
    let max = temporal::combine_echoes(&layers, 1);
    assert!(close(max.get(0, 0)[0], 0.3, 1e-6) && close(max.get(0, 0)[1], 0.4, 1e-6));
    let back = temporal::combine_echoes(&layers, 4);
    assert_eq!(back.get(0, 0), a.get(0, 0), "the current frame stays in front");
}

/// A clip environment over a synthetic moving picture (for the analysis effects).
struct ClipEnv {
    w: usize,
    h: usize,
    fps: f64,
    secs: f64,
    now: f64,
    key: u64,
    seq: (u32, u32),
    pic: fn(f64, f64, f64) -> [f32; 3],
}
impl ClipEnv {
    fn render(&self, t: f64, scale: f32) -> Image {
        let (w, h) = (((self.w as f32 * scale).round() as usize).max(1), ((self.h as f32 * scale).round() as usize).max(1));
        let k = self.w as f64 / w as f64;
        let mut img = Image::new(w, h);
        img.px.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
            for x in 0..w {
                let c = (self.pic)((x as f64 + 0.5) * k, (y as f64 + 0.5) * k, t);
                row[x * 4..x * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 1.0]);
            }
        });
        img
    }
}
impl FxEnv for ClipEnv {
    fn frame(&self, dt: f64, _: &EffectInstance) -> Option<Image> {
        Some(self.render(((self.now + dt) * self.fps).round() / self.fps, 1.0))
    }
    fn source_frame(&self, dt: f64, scale: f32) -> Option<Image> {
        Some(self.render(((self.now + dt) * self.fps).round() / self.fps, scale))
    }
    fn track(&self, _: usize) -> Option<Image> {
        None
    }
    fn layer_to_output(&self) -> Affine {
        Affine::IDENTITY
    }
    fn clip_seconds(&self) -> f64 {
        self.secs
    }
    fn clip_offset(&self) -> f64 {
        self.now
    }
    fn frame_rate(&self) -> f64 {
        self.fps
    }
    fn media_timecode(&self) -> String {
        String::new()
    }
    fn file_name(&self) -> String {
        String::new()
    }
    fn sequence_name(&self) -> String {
        String::new()
    }
    fn sequence_size(&self) -> (u32, u32) {
        self.seq
    }
    fn source_size(&self) -> (u32, u32) {
        (self.w as u32, self.h as u32)
    }
    fn clip_key(&self) -> u64 {
        self.key
    }
}

fn texture(x: f64, y: f64) -> [f32; 3] {
    let v = 0.5 + 0.45 * fbm(x as f32 / 9.0, y as f32 / 9.0, 0.3, 4.0, 11);
    let l = filmcraft_color::srgb_to_linear(v.clamp(0.0, 1.0));
    [l, l, l]
}
/// Camera jitter at time t: a few pixels, changing every frame.
fn jitter(t: f64) -> (f64, f64) {
    let f = (t * 24.0).round() as i64;
    ((hash01(f, 0, 0, 3) as f64 - 0.5) * 8.0, (hash01(f, 1, 0, 3) as f64 - 0.5) * 8.0)
}
fn shaky(x: f64, y: f64, t: f64) -> [f32; 3] {
    let (dx, dy) = jitter(t);
    texture(x + dx, y + dy)
}

#[test]
fn posterize_time_and_echo_read_other_frames() {
    fn moving(x: f64, _y: f64, t: f64) -> [f32; 3] {
        let v = if (x - t * 100.0).abs() < 4.0 { 1.0 } else { 0.0 };
        [v, v, v]
    }
    let mk = |now: f64| ClipEnv { w: 64, h: 8, fps: 24.0, secs: 1.0, now, key: 77, seq: (64, 8), pic: moving };
    // 12 fps: frames 0.0 and 1/24 show the same picture; 2/24 is new
    let e = inst("posterize_time");
    let at = |now: f64| {
        let env = mk(now);
        let mut img = env.render(now, 1.0);
        crate::effects::apply(&mut img, &e, &FxCtx { seconds: now, env: Some(&env), ..cx() });
        img
    };
    assert_eq!(at(0.0).px, at(1.0 / 24.0).px);
    assert_ne!(at(0.0).px, at(2.0 / 24.0).px);
    // Echo (one echo a frame back, Add): two bars
    let env = mk(0.5);
    let mut img = env.render(0.5, 1.0);
    let mut e = inst("echo");
    setf(&mut e, "time", -0.1);
    setf(&mut e, "decay", 0.5);
    crate::effects::apply(&mut img, &e, &FxCtx { seconds: 0.5, env: Some(&env), ..cx() });
    assert!(close(img.get(50, 4)[0], 1.0, 1e-5), "current bar {:?}", img.get(50, 4));
    assert!(close(img.get(40, 4)[0], 0.5, 1e-5), "echo bar {:?}", img.get(40, 4));
}

#[test]
fn warp_stabilizer_removes_camera_jitter() {
    let (w, h) = (192, 128);
    let mk = |now: f64| ClipEnv { w, h, fps: 24.0, secs: 1.0, now, key: 0xC0FFEE, seq: (w as u32, h as u32), pic: shaky };
    let mut e = inst("warp_stabilizer");
    set(&mut e, "result", ParamValue::Choice(1)); // No Motion
    set(&mut e, "framing", ParamValue::Choice(0)); // Stabilize Only
    clear_stabilizer_cache();
    let frames: Vec<(Image, Image)> = [3usize, 9, 15, 21]
        .iter()
        .map(|&f| {
            let now = f as f64 / 24.0;
            let env = mk(now);
            let raw = env.render(now, 1.0);
            let mut st = raw.clone();
            crate::effects::apply(&mut st, &e, &FxCtx { seconds: now, env: Some(&env), ..cx() });
            (raw, st)
        })
        .collect();
    // compare the interior of every pair of frames: stabilised frames agree, raw ones don't
    let interior = |a: &Image, b: &Image| {
        let mut s = 0.0f64;
        let mut n = 0;
        for y in 24..h - 24 {
            for x in 24..w - 24 {
                s += (a.get(x, y)[0] - b.get(x, y)[0]).abs() as f64;
                n += 1;
            }
        }
        s / n as f64
    };
    let (mut raw_err, mut st_err) = (0.0, 0.0);
    for i in 1..frames.len() {
        raw_err += interior(&frames[0].0, &frames[i].0);
        st_err += interior(&frames[0].1, &frames[i].1);
    }
    assert!(st_err < raw_err * 0.25, "stabilised {st_err} vs raw {raw_err}");
    // Auto-scale framing hides the borders
    set(&mut e, "framing", ParamValue::Choice(2));
    let env = mk(9.0 / 24.0);
    let mut st = env.render(9.0 / 24.0, 1.0);
    crate::effects::apply(&mut st, &e, &FxCtx { seconds: 9.0 / 24.0, env: Some(&env), ..cx() });
    assert!(st.px.as_chunks::<4>().0.iter().all(|p| p[3] > 0.99), "auto-scale leaves no border");
    // the path cache is reused
    assert!(stabilizer_path(&env, 0.0, filmcraft_project::TrackMethod::Position, false).is_some());
}

#[test]
fn auto_reframe_follows_the_subject() {
    fn subject(x: f64, y: f64, _t: f64) -> [f32; 3] {
        // a textured blob on the right third of a flat frame
        let d = ((x - 150.0).powi(2) + (y - 45.0).powi(2)).sqrt();
        if d < 20.0 { texture(x, y) } else { [0.2, 0.2, 0.2] }
    }
    // 16:9 source in a 9:16 sequence → zoom to fill the height and pan to the subject
    let env = ClipEnv { w: 160, h: 90, fps: 24.0, secs: 1.0, now: 0.5, key: 0xBEEF, seq: (90, 160), pic: subject };
    let mut img = env.render(0.5, 1.0);
    crate::effects::apply(&mut img, &inst("auto_reframe"), &FxCtx { seconds: 0.5, env: Some(&env), ..cx() });
    // the layer centre now shows the subject side of the frame (texture, not the flat grey)
    let c = img.get(80, 45);
    assert!((c[0] - 0.2).abs() > 1e-3 || (img.get(85, 40)[0] - 0.2).abs() > 1e-3, "{c:?}");
    // same aspect → nothing to do
    let env = ClipEnv { seq: (160, 90), key: 0xBEF0, ..env };
    let raw = env.render(0.5, 1.0);
    let mut img = raw.clone();
    crate::effects::apply(&mut img, &inst("auto_reframe"), &FxCtx { seconds: 0.5, env: Some(&env), ..cx() });
    assert_eq!(img.px, raw.px);
}

// ------------------------------------------------------------------ Motion Extract

use temporal::{ExtractMode, ExtractParams};

/// The settings that reproduce the first Reveal exactly: no gamma, no knee, no auto white.
fn mx(mode: ExtractMode) -> ExtractParams {
    ExtractParams { delay: 4, mode, gain: 1.0, black: 0.0, palette: 0, base_dim: 0.85, gamma: 1.0, knee: 0.0, auto_white: false }
}

/// `w`×`h` black with hard-edged discs `(centre x, centre y, radius, display-encoded value)`.
fn discs(w: usize, h: usize, list: &[(f32, f32, f32, f32)]) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut v = 0.0;
            for &(px, py, r, value) in list {
                if in_disc(x, y, (px, py, r)) {
                    v = value;
                }
            }
            let l = dec([v; 3]);
            img.px[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[l[0], l[1], l[2], 1.0]);
        }
    }
    img
}
fn in_disc(x: usize, y: usize, (px, py, r): (f32, f32, f32)) -> bool {
    let (dx, dy) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
    dx * dx + dy * dy <= r * r
}
fn extract(now: &Image, past: &Image, p: &ExtractParams) -> Image {
    let mut o = now.clone();
    temporal::extract_motion(&mut o, Some(past), p);
    o
}

#[test]
fn motion_extract_static_picture_gives_mid_grey_or_black() {
    let still = picture(48, 32);
    // Classic: whatever the picture, nothing moved, so every channel is 0.5 (display-encoded)
    for past in [Some(&still), None] {
        let mut o = still.clone();
        temporal::extract_motion(&mut o, past, &mx(ExtractMode::Classic));
        for y in 0..o.h {
            for x in 0..o.w {
                let c = enc_px(&o, x, y);
                assert!(c[..3].iter().all(|v| close(*v, 0.5, 1e-4)) && c[3] == 1.0, "({x},{y}) {c:?}");
            }
        }
    }
    // Reveal with the picture fully dimmed: black
    let black = extract(&still, &still, &ExtractParams { base_dim: 1.0, ..mx(ExtractMode::Reveal) });
    assert!(black.px.as_chunks::<4>().0.iter().all(|p| p[..3].iter().all(|v| v.abs() <= 1e-6) && p[3] == 1.0));
    // Reveal at the default dimming: a 15 % copy of the picture under no motion
    let dim = extract(&still, &still, &mx(ExtractMode::Reveal));
    for (x, y) in [(0, 0), (11, 7), (30, 20), (47, 31)] {
        let (a, b) = (enc_px(&dim, x, y), enc_px(&still, x, y));
        assert!((0..3).all(|k| close(a[k], b[k] * 0.15, 1e-4)), "({x},{y}) {a:?} vs {b:?}");
    }
}

#[test]
fn motion_extract_a_moving_disc_lights_its_new_and_old_places() {
    let (w, h) = (64, 40);
    let (a, b, still) = ((40.0, 20.0, 4.0), (24.0, 20.0, 4.0), (10.0, 30.0, 5.0));
    let now = discs(w, h, &[(a.0, a.1, a.2, 1.0), (still.0, still.1, still.2, 0.8)]);
    let past = discs(w, h, &[(b.0, b.1, b.2, 1.0), (still.0, still.1, still.2, 0.8)]);
    // Classic: white where the disc is, black where it was, mid-grey everywhere else (the still disc too)
    let o = extract(&now, &past, &mx(ExtractMode::Classic));
    let (mut at_a, mut at_b, mut lit) = (0, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let want = if in_disc(x, y, a) {
                at_a += 1;
                1.0
            } else if in_disc(x, y, b) {
                at_b += 1;
                0.0
            } else {
                0.5
            };
            let c = enc_px(&o, x, y);
            assert!(c[..3].iter().all(|v| close(*v, want, 1e-4)), "({x},{y}) want {want}, got {c:?}");
            lit += usize::from((c[0] - 0.5).abs() > 1e-3);
        }
    }
    assert!(at_a > 20 && at_a == at_b && lit == at_a + at_b, "{at_a} {at_b} {lit}");
    // Reveal, picture fully dimmed, Mono: white at both places (the change is |now - past|), black elsewhere
    let r = extract(&now, &past, &ExtractParams { base_dim: 1.0, ..mx(ExtractMode::Reveal) });
    for y in 0..h {
        for x in 0..w {
            let want = if in_disc(x, y, a) || in_disc(x, y, b) { 1.0 } else { 0.0 };
            let c = enc_px(&r, x, y);
            assert!(c[..3].iter().all(|v| close(*v, want, 1e-4)), "reveal ({x},{y}) want {want}, got {c:?}");
        }
    }
    // Reveal at the default dimming keeps the picture under the quiet places: the still disc at 15 %
    let d = extract(&now, &past, &mx(ExtractMode::Reveal));
    assert!(close(enc_px(&d, 10, 30)[0], 0.8 * 0.15, 1e-4) && close(enc_px(&d, 2, 2)[0], 0.0, 1e-5));
}

#[test]
fn motion_extract_gain_scales_the_change_linearly() {
    let (w, h) = (64, 40);
    let now = discs(w, h, &[(40.0, 20.0, 4.0, 0.3)]);
    let past = discs(w, h, &[(24.0, 20.0, 4.0, 0.3)]);
    // Classic: the deviation from mid-grey is gain × 0.5 × the change (0.3), up and down
    let dev = |gain: f32, x: usize, y: usize| enc_px(&extract(&now, &past, &ExtractParams { gain, ..mx(ExtractMode::Classic) }), x, y)[1] - 0.5;
    let unit = dev(1.0, 40, 20);
    assert!(close(unit, 0.15, 1e-4) && close(dev(1.0, 24, 20), -0.15, 1e-4), "{unit}");
    for g in [0.1, 0.5, 2.0, 3.0] {
        assert!(close(dev(g, 40, 20), g * unit, 1e-4), "gain {g}: {} vs {}", dev(g, 40, 20), g * unit);
        assert!(close(dev(g, 24, 20), -g * unit, 1e-4), "gain {g} (old place)");
    }
    // Reveal: the amount (before the ramp) is linear in gain until it clips at 1
    let amount = |gain: f32, diff: f32| temporal::motion_amount(diff, &ExtractParams { gain, ..mx(ExtractMode::Reveal) });
    assert!(close(amount(1.0, 0.2), 0.2, 1e-6));
    for g in [0.5, 2.0, 4.0] {
        assert!(close(amount(g, 0.2), g * amount(1.0, 0.2), 1e-6), "gain {g}");
    }
    assert_eq!(amount(8.0, 0.2), 1.0);
    assert_eq!(amount(2.0, -0.2), amount(2.0, 0.2), "the sign of the change does not matter");
}

#[test]
fn motion_extract_black_level_is_a_dead_zone() {
    let (w, h) = (64, 40);
    let past = discs(w, h, &[]);
    let p = ExtractParams { black: 0.2, ..mx(ExtractMode::Classic) };
    let at = |v: f32| enc_px(&extract(&discs(w, h, &[(40.0, 20.0, 4.0, v)]), &past, &p), 40, 20)[0];
    assert!(close(at(0.15), 0.5, 1e-4), "below the dead zone: {}", at(0.15));
    assert!(close(at(0.5), 0.5 + 0.5 * 0.375, 1e-4), "(0.5 - 0.2) / 0.8 of the change: {}", at(0.5));
    assert!(close(at(1.0), 1.0, 1e-4), "a full-scale change stays full scale: {}", at(1.0));
    let r = ExtractParams { black: 0.2, ..mx(ExtractMode::Reveal) };
    assert_eq!(temporal::motion_amount(0.15, &r), 0.0);
    assert!(close(temporal::motion_amount(0.5, &r), 0.375, 1e-6) && close(temporal::motion_amount(1.0, &r), 1.0, 1e-6));
}

#[test]
fn motion_extract_palettes_are_the_demo_ramps() {
    // the grid demo's PALS (Sorbet, Lagoon, Peach fuzz), copied from grid-demo/index.html
    let demo: [[&str; 5]; 3] = [
        ["#2c2745", "#6d5fa8", "#e49bc4", "#ffc9a8", "#fff5cf"],
        ["#17323f", "#3c8793", "#86d3bf", "#cdeeb0", "#fff7d8"],
        ["#3a2c3b", "#9b5f8e", "#f09aa0", "#ffd09a", "#fffae8"],
    ];
    let rgb = |s: &str| [1, 3, 5].map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap() as f32 / 255.0);
    for (i, stops) in demo.iter().enumerate() {
        for (k, hex) in stops.iter().enumerate() {
            let got = temporal::ramp_color(i as u32 + 1, k as f32 / 4.0);
            assert!(got.iter().zip(rgb(hex)).all(|(g, w)| close(*g, w, 1e-5)), "pastel {} stop {k}: {got:?} vs {hex}", i + 1);
        }
        let y: Vec<f32> = (0..=8).map(|k| luma(temporal::ramp_color(i as u32 + 1, k as f32 / 8.0))).collect();
        assert!(y.windows(2).all(|p| p[1] > p[0]), "pastel {} gets lighter along the ramp: {y:?}", i + 1);
    }
    // Mono (MotionScope): black, a cool grey at 0.45, white; positions off the ramp stay on its ends
    assert_eq!(temporal::ramp_color(0, 0.0), [0.0; 3]);
    assert!(temporal::ramp_color(0, 0.45).iter().zip([0.55, 0.57, 0.62]).all(|(g, w)| close(*g, w, 1e-6)));
    let white = |c: [f32; 3]| c.iter().all(|v| close(*v, 1.0, 1e-6));
    assert!(white(temporal::ramp_color(0, 1.0)) && white(temporal::ramp_color(0, 7.0)));
    assert_eq!(temporal::ramp_color(0, f32::NAN), [0.0; 3]);
    assert!(white(temporal::ramp_color(99, 1.0)), "an unknown palette is Mono");
    // Reveal in Pastel 1 at full signal ends on the cream end of the ramp
    let (w, h) = (64, 40);
    let r = extract(&discs(w, h, &[(40.0, 20.0, 4.0, 1.0)]), &discs(w, h, &[]), &ExtractParams { palette: 1, base_dim: 1.0, ..mx(ExtractMode::Reveal) });
    let c = enc_px(&r, 40, 20);
    assert!((0..3).all(|k| close(c[k], rgb("#fff5cf")[k], 1e-4)), "{c:?}");
    // Classic in Pastel 1: nothing moved is the middle stop
    let still = discs(w, h, &[(10.0, 10.0, 3.0, 0.6)]);
    let c = enc_px(&extract(&still, &still, &ExtractParams { palette: 1, ..mx(ExtractMode::Classic) }), 30, 30);
    assert!((0..3).all(|k| close(c[k], rgb("#e49bc4")[k], 1e-4)), "{c:?}");
}

/// A clip with a white bar moving right at 100 px/s over black (2 s, 24 fps).
fn bar_env(now: f64) -> ClipEnv {
    fn moving(x: f64, _y: f64, t: f64) -> [f32; 3] {
        let v = if (x - t * 100.0).abs() < 4.0 { 1.0 } else { 0.0 };
        [v, v, v]
    }
    ClipEnv { w: 128, h: 8, fps: 24.0, secs: 2.0, now, key: 5, seq: (128, 8), pic: moving }
}

#[test]
fn motion_extract_reads_the_past_through_the_clip_environment() {
    let env = bar_env(0.5);
    let now = env.render(0.5, 1.0);
    let run_at = |e: &EffectInstance| {
        let mut o = now.clone();
        crate::effects::apply(&mut o, e, &FxCtx { seconds: 0.5, env: Some(&env), ..cx() });
        o
    };
    let value = |o: &Image, x: usize| enc_px(o, x, 4)[0];
    // Delay 4 frames at 24 fps = 1/6 s: the bar is at x = 50 now and was at x = 33
    let e = inst("motion_extract");
    let o = run_at(&e);
    assert!(close(value(&o, 50), 1.0, 1e-4) && close(value(&o, 33), 0.0, 1e-4), "{} {}", value(&o, 50), value(&o, 33));
    assert!(close(value(&o, 5), 0.5, 1e-4) && close(value(&o, 100), 0.5, 1e-4));
    // Delay 8 frames: it was at x = 17
    let mut e8 = e.clone();
    setf(&mut e8, "delay_frames", 8.0);
    let o = run_at(&e8);
    assert!(close(value(&o, 17), 0.0, 1e-4) && close(value(&o, 33), 0.5, 1e-4) && close(value(&o, 50), 1.0, 1e-4));
    // Reveal, picture fully dimmed: white at both places
    let mut er = e.clone();
    set(&mut er, "mode", ParamValue::Choice(1));
    setf(&mut er, "base_dim", 1.0);
    let o = run_at(&er);
    assert!(close(value(&o, 50), 1.0, 1e-4) && close(value(&o, 33), 1.0, 1e-4) && close(value(&o, 5), 0.0, 1e-4));
    // Without an environment (adjustment layers, isolated use) the picture is left alone
    let mut o = now.clone();
    crate::effects::apply(&mut o, &e, &cx());
    assert_eq!(o.px, now.px);
    // A disabled effect does nothing
    let mut off = e.clone();
    off.enabled = false;
    assert_eq!(run_at(&off).px, now.px);
    assert!(is_temporal("motion_extract"));
}

#[test]
fn motion_extract_survives_hostile_parameters() {
    let env = bar_env(0.5);
    let now = env.render(0.5, 1.0);
    let check = |e: &EffectInstance, what: String| {
        let mut o = now.clone();
        crate::effects::apply(&mut o, e, &FxCtx { seconds: 0.5, env: Some(&env), ..cx() });
        assert!(o.px.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0 + 1e-4), "{what}");
    };
    for id in ["delay_frames", "gain", "black_level", "base_dim", "knee", "gamma"] {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -5.0, 1e30] {
            for mode in 0..2 {
                let mut e = inst("motion_extract");
                setf(&mut e, id, v);
                set(&mut e, "mode", ParamValue::Choice(mode));
                check(&e, format!("{id}={v} mode {mode}"));
            }
        }
    }
    for (mode, palette) in [(99, 0), (0, 99), (1, 99), (u32::MAX, u32::MAX)] {
        let mut e = inst("motion_extract");
        set(&mut e, "mode", ParamValue::Choice(mode));
        set(&mut e, "palette", ParamValue::Choice(palette));
        check(&e, format!("mode {mode} palette {palette}"));
    }
    for auto in [false, true] {
        let mut e = inst("motion_extract");
        set(&mut e, "mode", ParamValue::Choice(1));
        set(&mut e, "auto_white", ParamValue::Bool(auto));
        check(&e, format!("auto_white {auto}"));
    }
}

// ---- Reveal: knee, gamma and auto white

/// The Reveal code as it was before knee, gamma and auto white, kept here as the reference for the
/// bit-exact test.
fn reveal_before_knee(now: &Image, past: &Image, p: &ExtractParams) -> Image {
    let enc_of = |q: &[f32; 4]| enc(Image::unpremul(*q)).map(|v| v.min(1.0));
    let gate = |x: f32| ((x.abs() - p.black).max(0.0) / (1.0 - p.black)).copysign(x);
    let keep = 1.0 - p.base_dim;
    let mut img = now.clone();
    for (o, q) in img.px.as_chunks_mut::<4>().0.iter_mut().zip(past.px.as_chunks::<4>().0) {
        let (n, b) = (enc_of(o), enc_of(q));
        let m = (gate((luma(n) - luma(b)).abs()) * p.gain).clamp(0.0, 1.0);
        let out = lerp3(n.map(|v| v * keep), temporal::ramp_color(p.palette, m), m);
        let lin = dec(out);
        let a = o[3];
        o[..3].copy_from_slice(&[lin[0] * a, lin[1] * a, lin[2] * a]);
    }
    img
}

/// `img` with every row rotated right by `dx` pixels.
fn rolled(img: &Image, dx: usize) -> Image {
    let mut o = img.clone();
    for row in o.px.chunks_mut(img.w * 4) {
        row.rotate_right(dx * 4);
    }
    o
}

/// `w`x`h` black with a Gaussian blob of display-encoded peak `amp` centred at `(cx, cy)` (pixel-centre units).
fn blob(w: usize, h: usize, (cx, cy): (f32, f32), sigma: f32, amp: f32) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let l = dec([amp * (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp(); 3]);
            img.px[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[l[0], l[1], l[2], 1.0]);
        }
    }
    img
}

/// The parameters of a fresh Motion Extract effect, in Reveal.
fn reveal_defaults() -> ExtractParams {
    ExtractParams { mode: ExtractMode::Reveal, ..ExtractParams::read(&inst("motion_extract"), &cx()) }
}

#[test]
fn motion_extract_reveal_defaults_are_knee_05_gamma_06_auto_white_on() {
    let p = ExtractParams::read(&inst("motion_extract"), &cx());
    assert_eq!((p.delay, p.mode, p.palette), (4, ExtractMode::Classic, 0));
    assert_eq!((p.gain, p.black, p.base_dim), (1.0, 0.0, 0.85));
    assert_eq!((p.knee, p.gamma, p.auto_white), (0.5, 0.6, true));
}

#[test]
fn motion_extract_classic_ignores_knee_gamma_and_auto_white() {
    let (now, past) = (picture(48, 32), rolled(&picture(48, 32), 3));
    let reference = extract(&now, &past, &mx(ExtractMode::Classic));
    for (gamma, knee, auto_white) in [(0.6, 0.15, true), (2.0, 1.0, true), (0.3, 0.0, false)] {
        let o = extract(&now, &past, &ExtractParams { gamma, knee, auto_white, ..mx(ExtractMode::Classic) });
        assert_eq!(o.px, reference.px, "gamma {gamma} knee {knee} auto {auto_white}");
    }
}

#[test]
fn motion_extract_reveal_without_gamma_knee_and_auto_white_is_bit_identical_to_the_first_version() {
    let (w, h) = (48, 32);
    let pairs = [
        (picture(w, h), rolled(&picture(w, h), 3)),
        (discs(w, h, &[(30.0, 16.0, 6.0, 0.9), (12.0, 10.0, 5.0, 0.4)]), discs(w, h, &[(26.0, 16.0, 6.0, 0.9), (12.0, 14.0, 5.0, 0.4)])),
        (picture(w, h), picture(w, h)),
    ];
    let mut compared = 0;
    for (now, past) in &pairs {
        for gain in [0.5, 1.0, 3.0, 8.0] {
            for black in [0.0, 0.05, 0.2] {
                for palette in 0..=3 {
                    for base_dim in [0.0, 0.85, 1.0] {
                        let p = ExtractParams { gain, black, palette, base_dim, ..mx(ExtractMode::Reveal) };
                        let (new, old) = (extract(now, past, &p), reveal_before_knee(now, past, &p));
                        assert!(
                            new.px.iter().map(|v| v.to_bits()).eq(old.px.iter().map(|v| v.to_bits())),
                            "gain {gain} black {black} palette {palette} dim {base_dim}"
                        );
                        compared += 1;
                    }
                }
            }
        }
    }
    assert_eq!(compared, 3 * 4 * 3 * 4 * 3);
}

#[test]
fn motion_extract_reveal_defaults_make_a_faint_drifting_blob_visible() {
    // amplitude 0.05 of full scale, sigma 10 px, drifting 1 px per frame: the frame 4 back (the default delay) has it 4 px left
    let (w, h, delay) = (128, 96, 4);
    let (centre, shift) = ((70.5, 48.0), delay as f32);
    let now = blob(w, h, centre, 10.0, 0.05);
    let past = blob(w, h, (centre.0 - shift, centre.1), 10.0, 0.05);
    let at = |amounts: &[f32]| amounts[48 * w + 70];
    let on = temporal::reveal_amounts(&now, Some(&past), &reveal_defaults());
    let peak = on.iter().copied().fold(0.0, f32::max);
    assert!(at(&on) >= 0.5 && peak >= 0.5, "defaults: centre {} peak {peak}", at(&on));
    // the same frames the way the first version showed them, even at the largest gain, stay dark
    for gain in [1.0, 8.0] {
        let old = temporal::reveal_amounts(&now, Some(&past), &ExtractParams { gain, ..mx(ExtractMode::Reveal) });
        let peak = old.iter().copied().fold(0.0, f32::max);
        assert!(at(&old) < 0.1 && peak < 0.5, "first version, gain {gain}: centre {} peak {peak}", at(&old));
    }
    // without the white point gamma and knee alone do not get the centre there
    let no_white = temporal::reveal_amounts(&now, Some(&past), &ExtractParams { auto_white: false, ..reveal_defaults() });
    assert!(at(&no_white) < 0.5, "auto white off: centre {}", at(&no_white));
    // and the colour that comes out is the ramp, not the dim plate: at knee 0.5 the centre is graded, well above the plate
    let o = extract(&now, &past, &ExtractParams { palette: 1, ..reveal_defaults() });
    let c = enc_px(&o, 70, 48);
    assert!(c[0] > 0.6 && c[1] > 0.5, "centre pixel {c:?}");
}

#[test]
fn motion_extract_reveal_static_frames_stay_black_with_auto_white() {
    let (w, h) = (48, 32);
    let still = picture(w, h);
    let p = reveal_defaults();
    // identical frames: every amount is 0 and the fully dimmed output is exactly black
    assert!(temporal::reveal_amounts(&still, Some(&still), &p).iter().all(|&m| m == 0.0));
    let o = extract(&still, &still, &ExtractParams { base_dim: 1.0, ..p });
    assert!(o.px.as_chunks::<4>().0.iter().all(|q| q[..3].iter().all(|v| *v == 0.0) && q[3] == 1.0));
    // frames that differ by rounding-level noise (1e-6 in display-encoded units): still black, because the floor keeps the white point at 0.02
    let noisy = {
        let mut o = still.clone();
        for (i, q) in o.px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let c = enc(Image::unpremul(*q));
            let n = if i % 2 == 0 { 1e-6 } else { -1e-6 };
            let l = dec(c.map(|v| (v + n).clamp(0.0, 1.0)));
            *q = [l[0], l[1], l[2], 1.0];
        }
        o
    };
    let amounts = temporal::reveal_amounts(&noisy, Some(&still), &p);
    let peak = amounts.iter().copied().fold(0.0, f32::max);
    assert!(peak < 0.01, "noise peak {peak}");
    // the white point of such frames is the floor itself
    let g: Vec<f32> = (0..1000).map(|i| (i % 7) as f32 * 1e-6).collect();
    assert_eq!(temporal::auto_white_point(&g), temporal::WHITE_FLOOR);
    assert_eq!(temporal::auto_white_point(&[0.0; 64]), temporal::WHITE_FLOOR);
    assert_eq!(temporal::auto_white_point(&[]), temporal::WHITE_FLOOR);
}

#[test]
fn motion_extract_auto_white_point_is_the_99_5th_percentile_with_a_floor() {
    // a ramp 0..0.4 of 10 000 values: the top 0.5 % (50 values) start at 0.398
    let ramp: Vec<f32> = (0..10_000).map(|i| i as f32 / 10_000.0 * 0.4).collect();
    let w = temporal::auto_white_point(&ramp);
    assert!(close(w, 0.398, 0.004), "{w}");
    // 0.4 % outliers do not move it, 0.6 % do
    let mut g = vec![0.03f32; 10_000];
    g[..40].fill(0.9);
    assert!(close(temporal::auto_white_point(&g), 0.03, 0.001), "{}", temporal::auto_white_point(&g));
    g[..60].fill(0.9);
    assert!(close(temporal::auto_white_point(&g), 0.9, 0.002), "{}", temporal::auto_white_point(&g));
    // below the floor the floor wins
    assert_eq!(temporal::auto_white_point(&vec![0.01; 500]), temporal::WHITE_FLOOR);
    // a 1080p-sized frame of mixed values gives the sorted percentile to within 1 %
    let mut big: Vec<f32> = (0..1920 * 1080).map(|i| ((i * 2654435761usize) % 100_003) as f32 / 100_003.0 * 0.2).collect();
    let est = temporal::auto_white_point(&big);
    big.sort_by(f32::total_cmp);
    let exact = big[big.len() - (big.len() as f64 * 0.005).ceil() as usize];
    assert!(close(est, exact, exact * 0.01), "{est} vs {exact}");
}

#[test]
fn motion_extract_knee_and_gamma_shape_the_amount() {
    let base = mx(ExtractMode::Reveal);
    let shape = |g: f32, white: f32, p: &ExtractParams| temporal::shape(g, white, p);
    assert_eq!(shape(0.3, 1.0, &base), 0.3, "everything off is the old linear ramp");
    assert!(close(shape(0.25, 1.0, &ExtractParams { gamma: 0.5, ..base }), 0.5, 1e-6));
    assert!(close(shape(0.5, 1.0, &ExtractParams { gamma: 2.0, ..base }), 0.25, 1e-6));
    assert!(close(shape(0.01, 0.02, &base), 0.5, 1e-6), "divided by the white point");
    assert_eq!(shape(0.25, 1.0, &ExtractParams { gamma: 0.5, gain: 2.0, ..base }), 1.0, "gain after gamma, clipped");
    let k = ExtractParams { knee: 0.15, ..base };
    assert_eq!(shape(0.0, 1.0, &k), 0.0);
    assert!(close(shape(0.075, 1.0, &k), 0.5, 1e-6) && shape(0.15, 1.0, &k) == 1.0 && shape(0.9, 1.0, &k) == 1.0);
    assert!(close(shape(0.025, 1.0, &ExtractParams { knee: 0.01, ..base }), 0.5, 1e-6), "knees under 0.05 act as 0.05");
    assert!(
        close(shape(0.5, 1.0, &ExtractParams { knee: 1.0, ..base }), 0.5, 1e-6) && close(shape(0.25, 1.0, &ExtractParams { knee: 1.0, ..base }), 0.15625, 1e-6)
    );
    let d = reveal_defaults();
    let mut last = 0.0;
    for i in 0..=1000 {
        let m = shape(i as f32 / 1000.0, temporal::WHITE_FLOOR, &d);
        assert!((0.0..=1.0).contains(&m) && m >= last, "not monotone at {i}: {m} after {last}");
        last = m;
    }
}

// ------------------------------------------------------------------ through the compositor

mod pipeline {
    use super::*;
    use crate::{RenderOptions, SourceMap, render_sequence};
    use filmcraft_media::generators::GeneratorSource;
    use filmcraft_media::{DemoScene, Generator, MediaSource, SharedSource};
    use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef, SequenceSettings, TrackKind};
    use filmcraft_time::{FrameRate, TICKS_PER_SECOND, TimeRange};

    fn add(p: &mut Project, map: &mut SourceMap, g: GeneratorSource) -> ItemId {
        let info = g.info().clone();
        let generator = g.generator.clone();
        let id = p.add_item(
            &info.name.clone(),
            Label::Iris,
            ItemKind::Media(MediaClip {
                media: MediaRef::Generator(generator),
                info,
                interpret: Default::default(),
                mark_in: None,
                mark_out: None,
                markers: vec![],
                offline: false,
                proxy: None,
                identity: None,
            }),
            None,
        );
        map.0.insert(id, Arc::new(g) as SharedSource);
        id
    }

    fn place(p: &mut Project, seq: ItemId, track: usize, item: ItemId, fx: Vec<EffectInstance>) {
        let r = FrameRate::FPS_24;
        let mut ti = p.make_track_item(item, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, r.tick_of(48)), r).unwrap();
        ti.scale_to_frame = true;
        for (i, e) in fx.into_iter().enumerate() {
            ti.effects.insert(i, e);
        }
        p.sequence_mut(seq).unwrap().video_tracks[track].items.push(ti);
    }

    #[test]
    fn temporal_and_track_effects_render_through_the_sequence() {
        let mut p = Project::new("vfx");
        let mut map = SourceMap::default();
        let ocean = add(&mut p, &mut map, GeneratorSource::demo(DemoScene::OceanSunset));
        let white = add(
            &mut p,
            &mut map,
            GeneratorSource::new(Generator::ColorMatte { color: [1.0, 1.0, 1.0, 1.0] }, 64, 36, FrameRate::FPS_24, Tick(10 * TICKS_PER_SECOND)),
        );
        let seq = p.new_sequence("s", SequenceSettings { width: 64, height: 36, frame_rate: FrameRate::FPS_24, ..Default::default() }, 2, 0, None);
        let mut pt = inst("posterize_time");
        setf(&mut pt, "rate", 6.0);
        place(&mut p, seq, 0, ocean, vec![pt]);
        let r = FrameRate::FPS_24;
        let a = render_sequence(&p, seq, r.tick_of(1), RenderOptions::default(), &map);
        let b = render_sequence(&p, seq, r.tick_of(3), RenderOptions::default(), &map);
        assert_eq!(a.px, b.px, "frames 1 and 3 share the 6 fps frame");
        let c0 = render_sequence(&p, seq, r.tick_of(0), RenderOptions::default(), &map);
        assert_eq!(a.px, c0.px);
        // Track Matte Key on V2 (white) using V1's luma
        let mut tm = inst("track_matte");
        set(&mut tm, "matte", ParamValue::Choice(1));
        set(&mut tm, "composite", ParamValue::Choice(1));
        place(&mut p, seq, 1, white, vec![tm]);
        let img = render_sequence(&p, seq, r.tick_of(0), RenderOptions::default(), &map);
        assert!(img.px.iter().all(|v| v.is_finite()));
        assert!(max_diff(&img, &c0) > 1e-3, "the matted white layer shows over V1");
        // Echo through the compositor stays finite and differs from the plain frame
        let mut p2 = Project::new("echo");
        let mut map2 = SourceMap::default();
        let ocean2 = add(&mut p2, &mut map2, GeneratorSource::demo(DemoScene::OceanSunset));
        let seq2 = p2.new_sequence("s", SequenceSettings { width: 64, height: 36, frame_rate: FrameRate::FPS_24, ..Default::default() }, 1, 0, None);
        let mut echo = inst("echo");
        setf(&mut echo, "time", -0.5);
        setf(&mut echo, "count", 2.0);
        set(&mut echo, "operator", ParamValue::Choice(6));
        place(&mut p2, seq2, 0, ocean2, vec![echo]);
        let e = render_sequence(&p2, seq2, r.tick_of(30), RenderOptions::default(), &map2);
        assert!(e.px.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn motion_extract_renders_through_the_sequence() {
        let r = FrameRate::FPS_24;
        let size = || SequenceSettings { width: 64, height: 36, frame_rate: r, ..Default::default() };
        // a still picture is mid-grey, also on the frames that have fewer than 4 frames behind them
        let mut p = Project::new("mx-still");
        let mut map = SourceMap::default();
        let matte = add(&mut p, &mut map, GeneratorSource::new(Generator::ColorMatte { color: [0.2, 0.5, 0.8, 1.0] }, 64, 36, r, Tick(10 * TICKS_PER_SECOND)));
        let seq = p.new_sequence("s", size(), 1, 0, None);
        place(&mut p, seq, 0, matte, vec![inst("motion_extract")]);
        for frame in [0, 2, 30] {
            let img = render_sequence(&p, seq, r.tick_of(frame), RenderOptions::default(), &map);
            for y in 0..img.h {
                for x in 0..img.w {
                    let c = enc_px(&img, x, y);
                    assert!(c[..3].iter().all(|v| close(*v, 0.5, 2e-3)), "frame {frame} ({x},{y}) {c:?}");
                }
            }
        }
        // a moving picture is not: the plasma differs from its plain frame and strays from mid-grey
        let mut p2 = Project::new("mx-plasma");
        let mut map2 = SourceMap::default();
        let plasma = add(&mut p2, &mut map2, GeneratorSource::demo(DemoScene::Plasma));
        let seq2 = p2.new_sequence("s", size(), 1, 0, None);
        let mut fx = inst("motion_extract");
        setf(&mut fx, "delay_frames", 12.0);
        place(&mut p2, seq2, 0, plasma, vec![fx]);
        let with = render_sequence(&p2, seq2, r.tick_of(30), RenderOptions::default(), &map2);
        let plain = render_sequence(&p2, seq2, r.tick_of(30), RenderOptions { effects: false, ..Default::default() }, &map2);
        assert!(with.px.iter().all(|v| v.is_finite()));
        assert!(max_diff(&with, &plain) > 0.05, "the effect changes the picture");
        let spread = (0..with.h).flat_map(|y| (0..with.w).map(move |x| (x, y))).map(|(x, y)| (enc_px(&with, x, y)[1] - 0.5).abs()).fold(0.0f32, f32::max);
        assert!(spread > 0.02, "the plasma moved in half a second: {spread}");
    }
}
