//! GL context capabilities the backend resolves once, at construction. [`classify`] is **pure**
//! (a function of the driver strings) so it is fully Tier-0 testable with no GL; [`probe`] is the
//! thin `unsafe` wrapper that reads those strings off a live context and hands them to `classify`.
//!
//! Two capabilities drive real decisions: the **shader dialect** ([`ShaderClass`]) selects the
//! `#version` header the one GLSL source is compiled under (DESIGN §8), and the **renderable float
//! format** ([`RenderableFloat`]) decides whether the linear-HDR scratch is `RGBA16F` or falls
//! back to `sRGB8_ALPHA8` on a WebGL2/GLES context lacking `EXT_color_buffer_float` (DESIGN §9).

use backdrop_blur_core::BlurError;
use glow::HasContext;

/// The GLSL dialect the shaders compile under — desktop vs embedded, which pick the `#version`
/// header (DESIGN §8). Mirrors `egui_glow`'s `ShaderVersion` split rather than reusing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderClass {
    /// Desktop OpenGL (3.3+): `#version 140`.
    GlDesktop,
    /// GLES 3.0 / WebGL2: `#version 300 es` + `precision highp` qualifiers.
    Es300,
}

/// The color-renderable float format for the linear scratch/grab textures. `RGBA16F` is preferred
/// (linear HDR); the `sRGB8_ALPHA8` fallback keeps the blur renderable on a WebGL2/GLES context
/// without `EXT_color_buffer_float` (no HDR headroom, but correct).
///
/// **The `sRGB8_ALPHA8` fallback's color-correctness rests on an implicit platform contract.** The
/// blur passes write *linear* values to the scratch and rely on the hardware to encode linear→sRGB
/// on write and decode sRGB→linear on the next sample (the perceptually-uniform 8-bit round-trip
/// DESIGN §9 describes). The sample-side decode is unconditional everywhere, but the *write-side
/// encode* to an sRGB attachment is automatic only where there is no `GL_FRAMEBUFFER_SRGB` enable to
/// gate it — i.e. **GLES 3.0 and WebGL2** (GLES 3.0.6 §4.1.8 has no such enable; the encode is
/// always-on for an sRGB target). On **desktop GL** that encode is gated on `GL_FRAMEBUFFER_SRGB`,
/// which is default-disabled and this crate never enables, so the round-trip would be gamma-broken
/// there. That case is unreachable by construction: `classify` only pairs `Srgb8Rgba8` with an
/// *embedded* profile (desktop GL has `RGBA16F` color-renderable in core, GL 3.3 §3.9.1), pinned by
/// a `debug_assert` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderableFloat {
    /// `RGBA16F` — linear HDR, color-renderable. Desktop GL 3.0+ always; GLES3/WebGL2 with
    /// `EXT_color_buffer_float`.
    Rgba16F,
    /// `sRGB8_ALPHA8` fallback when float-render is unavailable. **Embedded (GLES/WebGL2) contexts
    /// only** — its linear round-trip is correct solely under their automatic sRGB encode-on-write
    /// (see the type-level note).
    Srgb8Rgba8,
}

/// The capabilities of a live GL context, resolved once at construction and read back per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlProfile {
    /// Which `#version` dialect the shaders compile under.
    pub shader_class: ShaderClass,
    /// Whether this is an embedded context (GLES / WebGL2) vs desktop GL.
    pub embedded: bool,
    /// Whether `GL_FRAMEBUFFER_SRGB` is a *valid* capability to query on this context — core on
    /// desktop GL, present on a GLES context only via `EXT_sRGB_write_control`, and absent on core
    /// GLES 3.0 / WebGL2 (where querying the enum raises `GL_INVALID_ENUM`). Gates whether the
    /// composite's encode resolver may consult the enable ([`crate::composite::resolve_target_encoding`]).
    pub srgb_enable_is_queryable: bool,
    /// The scratch/grab float format this context can render to.
    pub renderable_float: RenderableFloat,
    /// The default framebuffer's sample count (`GL_SAMPLES`); `> 0` means the grab must resolve
    /// MSAA before sampling (DESIGN §11, IMPL §2c). Clamped non-negative.
    pub samples: i32,
}

impl GlProfile {
    /// Read the capabilities off a **live, current** GL context. Returns
    /// [`BlurError::UnsupportedContext`] when the context's `GL_VERSION` names an API level below
    /// the backend's minimum (desktop GL 3.3, GLES 3.0, WebGL 2.0).
    pub fn probe(gl: &glow::Context) -> Result<Self, BlurError> {
        // SAFETY: `gl` is a live, current GL context (the backend's construction-time contract —
        // probe runs only from the host's eframe/test context while it is current). These are
        // pure state queries: `get_parameter_string`/`get_parameter_i32` read driver strings and
        // integers, taking no caller pointers and mutating no GL state, so they are sound for any
        // current context.
        let (version, samples) = unsafe {
            (
                gl.get_parameter_string(glow::VERSION),
                gl.get_parameter_i32(glow::SAMPLES),
            )
        };
        // `supported_extensions` is glow's safe, cached accessor (no GL call).
        let extensions: Vec<&str> = gl
            .supported_extensions()
            .iter()
            .map(String::as_str)
            .collect();
        classify(&version, &extensions, samples)
    }
}

/// The API family a `GL_VERSION` string names — the *flavor* half of classification, deliberately
/// independent of the version-number parse (see the invariant note in [`classify`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlFlavor {
    /// Desktop OpenGL — a bare `"major.minor …"` version string.
    Desktop,
    /// OpenGL ES (`"OpenGL ES major.minor …"`, including the `"OpenGL ES-CM …"` common profile).
    Embedded,
    /// WebGL (`"WebGL major.minor (…)"`) — checked before `OpenGL ES` because the WebGL2 string
    /// embeds a GLES parenthetical.
    WebGl,
}

impl GlFlavor {
    /// The minimum `(major, minor)` the grab-pass backend requires for this flavor: desktop GL
    /// 3.3, GLES 3.0, WebGL 2.0 (the contexts whose dialects the shaders are written for).
    fn minimum(self) -> (u32, u32) {
        match self {
            GlFlavor::Desktop => (3, 3),
            GlFlavor::Embedded => (3, 0),
            GlFlavor::WebGl => (2, 0),
        }
    }

    /// The family name a [`BlurError::UnsupportedContext`] detail uses for this flavor.
    fn family_name(self) -> &'static str {
        match self {
            GlFlavor::Desktop => "OpenGL",
            GlFlavor::Embedded => "OpenGL ES",
            GlFlavor::WebGl => "WebGL",
        }
    }
}

/// The flavor of a `GL_VERSION` string, by substring precedence: `"WebGL"` first (the WebGL2
/// string `"WebGL 2.0 (OpenGL ES 3.0 …)"` also contains `"OpenGL ES"`), then `"OpenGL ES"`, else
/// desktop.
fn flavor_of(version: &str) -> GlFlavor {
    if version.contains("WebGL") {
        GlFlavor::WebGl
    } else if version.contains("OpenGL ES") {
        GlFlavor::Embedded
    } else {
        GlFlavor::Desktop
    }
}

/// The first `major.minor` numeric token anywhere in a `GL_VERSION` string: `"4.6.0 NVIDIA …"` →
/// `(4, 6)`, `"OpenGL ES 3.0 Mesa"` → `(3, 0)`, `"WebGL 2.0 (OpenGL ES 3.0 …)"` → `(2, 0)`,
/// `"OpenGL ES-CM 1.1"` → `(1, 1)`. `None` when no such token exists — a spec-violating driver
/// string; the caller then skips the version gate rather than guessing.
fn parse_major_minor(version: &str) -> Option<(u32, u32)> {
    version
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .find_map(|token| {
            let (major, rest) = token.split_once('.')?;
            let minor = rest.split_once('.').map_or(rest, |(m, _)| m);
            Some((major.parse().ok()?, minor.parse().ok()?))
        })
}

/// Classify a context from its `GL_VERSION` string, extension set, and sample count — **pure**, so
/// every branch is Tier-0 testable. The GLSL version is not an input: the dialect follows directly
/// from the string's flavor (`"WebGL"` / `"OpenGL ES"` / desktop, see [`flavor_of`]), and the
/// `#version` header is then fixed. Returns [`BlurError::UnsupportedContext`] when the string
/// names an API level below the backend's minimum (desktop GL 3.3, GLES 3.0, WebGL 2.0), so a
/// too-old context fails at construction with the real diagnosis instead of dying later as an
/// unrelated shader-compile error.
pub fn classify(version: &str, extensions: &[&str], samples: i32) -> Result<GlProfile, BlurError> {
    // INVARIANT: flavor and version number are two independent decisions, deliberately. A
    // malformed GL_VERSION string can degrade the *gate* (the number fails to parse and the gate
    // is skipped, below) but can never flip the *shader dialect* — a dialect flip would send
    // `#version 140` to a WebGL2 context, resurfacing the problem as an unrelated shader-compile
    // error (the wrong-diagnosis bug this gate exists to kill).
    let flavor = flavor_of(version);
    // Gate: a below-minimum context is rejected here, at construction. A parseable flavor whose
    // number does NOT parse proceeds UNGATED under the flavor-correct dialect: such a string
    // violates the GL spec's version format, so it comes from an exotic driver, and refusing a
    // driver that may work would be a regression. The residual — an old such driver still dying
    // as a shader-compile error — is exactly today's behavior.
    if let Some((major, minor)) = parse_major_minor(version) {
        let (req_major, req_minor) = flavor.minimum();
        if (major, minor) < (req_major, req_minor) {
            let family = flavor.family_name();
            return Err(BlurError::UnsupportedContext {
                detail: format!(
                    "the grab-pass backend requires {family} {req_major}.{req_minor} or newer, found \
                     {family} {major}.{minor} (GL_VERSION \"{version}\")"
                ),
            });
        }
    }
    // Embedded (GLES ∪ WebGL) drives the shader class and the float-format fallback below.
    let embedded = matches!(flavor, GlFlavor::Embedded | GlFlavor::WebGl);
    let shader_class = if embedded {
        ShaderClass::Es300
    } else {
        ShaderClass::GlDesktop
    };
    // RGBA16F is color-renderable in desktop GL 3.0+ unconditionally (GL 3.3 §3.9.1). On GLES3/WebGL2
    // it requires EXT_color_buffer_float; without it, fall back to sRGB8_ALPHA8 (renderable
    // everywhere). The extension may appear with or without the `GL_` prefix depending on the driver.
    let has_float_render = extensions
        .iter()
        .any(|e| *e == "EXT_color_buffer_float" || *e == "GL_EXT_color_buffer_float");
    // The `!embedded` arm is load-bearing for *color-correctness*, not just preference: the
    // sRGB8_ALPHA8 fallback's linear round-trip is correct only under the automatic sRGB
    // encode-on-write of GLES 3.0 / WebGL2 (no GL_FRAMEBUFFER_SRGB to gate it). On desktop GL that
    // encode is gated and default-off, so desktop must never take the fallback — and need not, since
    // it always has RGBA16F renderable. See the `RenderableFloat` type note.
    let renderable_float = if !embedded || has_float_render {
        RenderableFloat::Rgba16F
    } else {
        RenderableFloat::Srgb8Rgba8
    };
    // Invariant (implication form): Srgb8Rgba8 ⟹ embedded. See the `RenderableFloat` type note.
    debug_assert!(
        !matches!(renderable_float, RenderableFloat::Srgb8Rgba8) || embedded,
        "the sRGB8_ALPHA8 fallback is color-correct only on embedded (GLES/WebGL2) contexts, where \
         sRGB encode-on-write is automatic; a desktop GL context must resolve to RGBA16F"
    );
    // `GL_FRAMEBUFFER_SRGB` is a core enable on desktop GL, exposed on a GLES context only via
    // `EXT_sRGB_write_control`, and absent on core GLES 3.0 / WebGL2 (querying it raises
    // `GL_INVALID_ENUM`). The extension may appear with or without the `GL_` prefix.
    let srgb_enable_is_queryable = !embedded
        || extensions
            .iter()
            .any(|e| *e == "EXT_sRGB_write_control" || *e == "GL_EXT_sRGB_write_control");
    // Invariant (implication form): desktop (!embedded) ⟹ the enable is queryable (core capability).
    // Mirrors the `renderable_float` implication above so a future edit to the `!embedded ||` term
    // cannot silently produce a desktop profile that skips the enable gate.
    debug_assert!(
        embedded || srgb_enable_is_queryable,
        "desktop GL always exposes GL_FRAMEBUFFER_SRGB as a core capability"
    );
    Ok(GlProfile {
        shader_class,
        embedded,
        srgb_enable_is_queryable,
        renderable_float,
        samples: samples.max(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_gl_is_not_embedded_and_renders_rgba16f() {
        let p = classify("4.6.0 NVIDIA 535.288.01", &[], 0).expect("classify");
        assert_eq!(p.shader_class, ShaderClass::GlDesktop);
        assert!(!p.embedded);
        // Desktop GL renders RGBA16F with no extension needed.
        assert_eq!(p.renderable_float, RenderableFloat::Rgba16F);
    }

    #[test]
    fn gles3_without_the_float_extension_falls_back_to_srgb8() {
        let p = classify("OpenGL ES 3.0 Mesa", &[], 0).expect("classify");
        assert_eq!(p.shader_class, ShaderClass::Es300);
        assert!(p.embedded);
        assert_eq!(p.renderable_float, RenderableFloat::Srgb8Rgba8);
    }

    #[test]
    fn gles3_with_the_float_extension_renders_rgba16f() {
        let p = classify("OpenGL ES 3.2 NVIDIA", &["EXT_color_buffer_float"], 0).expect("classify");
        assert_eq!(p.renderable_float, RenderableFloat::Rgba16F);
        let prefixed =
            classify("OpenGL ES 3.2 NVIDIA", &["GL_EXT_color_buffer_float"], 0).expect("classify");
        assert_eq!(prefixed.renderable_float, RenderableFloat::Rgba16F);
    }

    #[test]
    fn webgl2_version_string_is_classified_embedded() {
        // glow's web backend reports VERSION as "WebGL 2.0 (OpenGL ES 3.0 Chromium)".
        let p = classify(
            "WebGL 2.0 (OpenGL ES 3.0 Chromium)",
            &["EXT_color_buffer_float"],
            0,
        )
        .expect("classify");
        assert_eq!(p.shader_class, ShaderClass::Es300);
        assert!(p.embedded);
        assert_eq!(p.renderable_float, RenderableFloat::Rgba16F);
    }

    #[test]
    fn the_srgb8_fallback_is_never_paired_with_a_desktop_profile() {
        // The safety invariant the fallback's color-correctness depends on (see RenderableFloat): the
        // sRGB8_ALPHA8 scratch is only ever selected for an embedded context, where sRGB
        // encode-on-write is automatic. A desktop string — with OR without the float extension —
        // must resolve to RGBA16F, never the fallback.
        for ext in [vec![], vec!["EXT_color_buffer_float"]] {
            let p = classify("4.6.0 NVIDIA 535.288.01", &ext, 0).expect("classify");
            assert!(!p.embedded);
            assert_eq!(p.renderable_float, RenderableFloat::Rgba16F);
        }
    }

    #[test]
    fn samples_pass_through_clamped_non_negative() {
        assert_eq!(classify("4.6.0", &[], 4).expect("classify").samples, 4);
        // A driver returning -1 (no MSAA query support) clamps to 0 (no resolve).
        assert_eq!(classify("4.6.0", &[], -1).expect("classify").samples, 0);
    }

    #[test]
    fn classify_rejects_gles2_naming_requirement_and_raw_string() {
        let err = classify("OpenGL ES 2.0 Mesa 20.0", &[], 0)
            .expect_err("GLES 2.0 is below the 3.0 minimum");
        let BlurError::UnsupportedContext { detail } = err else {
            panic!("expected UnsupportedContext, got {err:?}");
        };
        assert!(
            detail.contains("OpenGL ES 3.0"),
            "detail names the requirement: {detail}"
        );
        assert!(
            detail.contains("\"OpenGL ES 2.0 Mesa 20.0\""),
            "detail quotes the raw version string: {detail}"
        );
    }

    #[test]
    fn classify_rejects_webgl1() {
        let err = classify("WebGL 1.0 (OpenGL ES 2.0 Chromium)", &[], 0)
            .expect_err("WebGL 1.0 is below 2.0");
        assert!(matches!(err, BlurError::UnsupportedContext { .. }));
    }

    #[test]
    fn classify_rejects_desktop_gl_2_1() {
        let err =
            classify("2.1 Mesa 10.1", &[], 0).expect_err("desktop GL 2.1 is below the 3.3 minimum");
        assert!(matches!(err, BlurError::UnsupportedContext { .. }));
    }

    #[test]
    fn classify_rejects_desktop_gl_3_2() {
        // Desktop is the only flavor whose minimum has a nonzero MINOR (3.3), so this is the one
        // case that pins the minor half of the gate: a comparison that ignores the minor, or a
        // gate misplaced at (3, 2), accepts this string and fails here.
        let err = classify("3.2.0 Mesa 20.0", &[], 0)
            .expect_err("desktop GL 3.2 is below the 3.3 minimum");
        assert!(matches!(err, BlurError::UnsupportedContext { .. }));
    }

    #[test]
    fn classify_rejects_gles_common_profile_1_1() {
        // "OpenGL ES-CM 1.1" still contains "OpenGL ES" (embedded flavor) and 1.1 is extractable,
        // so this is a rejection case, not an ungated tolerance case.
        let err =
            classify("OpenGL ES-CM 1.1", &[], 0).expect_err("GLES-CM 1.1 is below the 3.0 minimum");
        assert!(matches!(err, BlurError::UnsupportedContext { .. }));
    }

    #[test]
    fn classify_accepts_desktop_gl_at_the_3_3_boundary() {
        let p = classify("3.3.0 NVIDIA 535.288.01", &[], 0).expect("desktop GL 3.3 is the minimum");
        assert_eq!(p.shader_class, ShaderClass::GlDesktop);
        assert!(!p.embedded);
    }

    #[test]
    fn classify_accepts_gles_at_the_3_0_boundary() {
        let p = classify("OpenGL ES 3.0 Mesa", &[], 0).expect("GLES 3.0 is the minimum");
        assert_eq!(p.shader_class, ShaderClass::Es300);
        assert!(p.embedded);
    }

    #[test]
    fn classify_accepts_webgl_at_the_2_0_boundary() {
        let p = classify("WebGL 2.0 (OpenGL ES 3.0 Chromium)", &[], 0)
            .expect("WebGL 2.0 is the minimum");
        assert!(p.embedded);
        assert_eq!(p.shader_class, ShaderClass::Es300);
    }

    #[test]
    fn classify_accepts_bare_webgl_2_0_keeping_the_embedded_dialect() {
        // The flavor decision survives a missing GLES parenthetical: "WebGL" alone is enough to
        // pin the embedded dialect.
        let p = classify("WebGL 2.0", &[], 0).expect("bare WebGL 2.0 is the minimum");
        assert!(p.embedded);
        assert_eq!(p.shader_class, ShaderClass::Es300);
    }

    #[test]
    fn classify_passes_a_numberless_string_ungated_as_desktop() {
        // No extractable major.minor: the gate is skipped (spec-violating driver), never guessed.
        let p = classify("Weird Custom Driver", &[], 0).expect("numberless strings pass ungated");
        assert_eq!(p.shader_class, ShaderClass::GlDesktop);
        assert!(!p.embedded);
    }

    #[test]
    fn classify_keeps_the_embedded_dialect_when_the_number_is_unparseable() {
        // A malformed string can degrade the gate but never flip the dialect: an embedded-flavored
        // string with no extractable number must stay Es300, never fall back to `#version 140`.
        let p =
            classify("OpenGL ES Mesa", &[], 0).expect("numberless embedded strings pass ungated");
        assert!(p.embedded);
        assert_eq!(p.shader_class, ShaderClass::Es300);
    }

    #[test]
    fn desktop_gl_can_always_query_the_srgb_enable() {
        // GL_FRAMEBUFFER_SRGB is core on desktop GL — no extension needed.
        let p = classify("4.6.0 NVIDIA 535.288.01", &[], 0).expect("classify");
        assert!(!p.embedded);
        assert!(p.srgb_enable_is_queryable);
    }

    #[test]
    fn core_gles3_and_webgl2_cannot_query_the_srgb_enable() {
        // Core GLES 3.0 / WebGL2 have no GL_FRAMEBUFFER_SRGB — querying it raises GL_INVALID_ENUM.
        let gles = classify("OpenGL ES 3.0 Mesa", &[], 0).expect("classify");
        assert!(gles.embedded);
        assert!(!gles.srgb_enable_is_queryable);
        let web = classify("WebGL 2.0 (OpenGL ES 3.0 Chromium)", &[], 0).expect("classify");
        assert!(web.embedded);
        assert!(!web.srgb_enable_is_queryable);
    }

    #[test]
    fn gles3_with_srgb_write_control_can_query_the_enable() {
        // A GLES context advertising EXT_sRGB_write_control exposes GL_FRAMEBUFFER_SRGB — with or
        // without the `GL_` prefix, mirroring the float-extension accessor.
        let p = classify("OpenGL ES 3.2 NVIDIA", &["EXT_sRGB_write_control"], 0).expect("classify");
        assert!(p.srgb_enable_is_queryable);
        let prefixed =
            classify("OpenGL ES 3.2 NVIDIA", &["GL_EXT_sRGB_write_control"], 0).expect("classify");
        assert!(prefixed.srgb_enable_is_queryable);
    }

    #[test]
    fn the_srgb_enable_is_always_queryable_on_a_desktop_profile() {
        // The invariant `debug_assert`ed in `classify`: desktop ⟹ queryable, independent of any
        // extension string. Mirrors `the_srgb8_fallback_is_never_paired_with_a_desktop_profile`.
        for ext in [
            vec![],
            vec!["EXT_sRGB_write_control"],
            vec!["GL_EXT_color_buffer_float"],
        ] {
            let p = classify("4.6.0 NVIDIA 535.288.01", &ext, 0).expect("classify");
            assert!(!p.embedded);
            assert!(p.srgb_enable_is_queryable);
        }
    }
}
