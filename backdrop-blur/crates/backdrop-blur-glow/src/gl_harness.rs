//! Shared, `gl-snapshots`-gated test harness: a **surfaceless headless GL context** for the Tier-1
//! readback tests. This is test scaffolding, not library code — `expect`/`eprintln!`/`transmute`
//! are deliberate here.
//!
//! Recipe (proven on this NVIDIA host; see the gotchas below):
//! ```text
//! load libEGL.so.1 (libloading) -> khronos_egl DynamicInstance<EGL1_4>
//!   -> eglQueryDevicesEXT            (raw FFI via eglGetProcAddress + transmute)
//!   -> eglGetPlatformDisplayEXT(EGL_PLATFORM_DEVICE_EXT, device)   (raw FFI)
//!   -> eglInitialize -> eglBindAPI(OPENGL) -> eglChooseConfig(PBUFFER_BIT, OPENGL_BIT, RGBA8)
//!   -> eglCreateContext(MAJOR=3, MINOR=3, NO profile mask)   (NVIDIA rejects the profile mask)
//!   -> eglMakeCurrent(NO_SURFACE, NO_SURFACE)                (EGL_KHR_surfaceless_context)
//!   -> glow::Context::from_loader_function_cstr(eglGetProcAddress)
//! ```
//!
//! Gotchas a reimplementation must keep:
//! - The DRI render node (`/dev/dri/renderD128`) is `root:render` and unreachable here, so the
//!   Mesa/gbm path is dead — the NVIDIA `EGL_EXT_platform_device` path is mandatory.
//! - `EGL_CONTEXT_OPENGL_PROFILE_MASK = CORE_PROFILE_BIT` makes `eglCreateContext` fail with
//!   `BadAttribute` on this NVIDIA libEGL. Pin the version with MAJOR/MINOR only.
//! - `eglQueryDevicesEXT` / `eglGetPlatformDisplayEXT` are not wrapped by khronos-egl — fetch via
//!   `get_proc_address` + `transmute` to the spec ABI (`extern "system"`).
//! - Load `DynamicInstance::<EGL1_4>` (every safe call is <= 1.4; the platform-display capability
//!   is reached through the EXT entry points).
//!
//! On this host the context comes back **desktop GL 3.3** (`#version 140` path). The ES/WebGL2
//! dialect is exercised by the Step-4 web tier, not here.
//!
//! A crate module (gated `gl-snapshots + not(wasm)`, doc-hidden `pub` at the `mod` decl) rather
//! than a `tests/` helper, so the Tier-1 **unit** tests can reach the `pub(crate)` blur internals
//! (`grab`, scratch eviction, composite) — an integration test in `tests/` could only see the
//! public API — and so the egui adapter's gated tests can reuse the EGL bring-up instead of
//! duplicating it.
#![allow(
    dead_code,
    reason = "test harness; a given build step's tests use a subset of the helper API"
)]

use std::ffi::{CStr, c_void};
use std::sync::Arc;

use glow::HasContext;
use khronos_egl as egl;

// EGL EXT/KHR constants khronos-egl does not export.
const EGL_PLATFORM_DEVICE_EXT: egl::Enum = 0x313F;
const EGL_CONTEXT_MAJOR_VERSION: egl::Int = 0x3098;
const EGL_CONTEXT_MINOR_VERSION: egl::Int = 0x30FB;

type Egl = egl::Instance<egl::Dynamic<libloading::Library, egl::EGL1_4>>;

// Spec ABIs for the two EXT entry points (EGL_EXT_device_enumeration / EGL_EXT_platform_device).
type PfnEglQueryDevicesExt = unsafe extern "system" fn(
    max_devices: egl::Int,
    devices: *mut *mut c_void,
    num_devices: *mut egl::Int,
) -> egl::Boolean;
type PfnEglGetPlatformDisplayExt = unsafe extern "system" fn(
    platform: egl::Enum,
    native_display: *mut c_void,
    attrib_list: *const egl::Int,
) -> egl::EGLDisplay;

/// A live, current, surfaceless GL context plus the EGL handles that own it. [`Deref`] yields the
/// `glow::Context`. **The crate under test owns its own GL objects and must `destroy` them before
/// this drops**; `Drop` here only tears down the EGL context/display. **Do not install a GL debug
/// callback through this context**: glow's native `Context` `Drop` then issues a real GL call
/// (`glDebugMessageCallback(None)`) — and that `Drop` runs after this harness has already called
/// `eglDestroyContext`, so it would issue GL against a destroyed context.
pub struct HeadlessGl {
    egl: Egl,
    display: egl::Display,
    context: egl::Context,
    gl: Arc<glow::Context>,
}

impl HeadlessGl {
    /// Hand the context to APIs that take `&Arc<glow::Context>` (e.g. `GrabPassRenderer::new`).
    ///
    /// Clones must **not** outlive this [`HeadlessGl`] — its `Drop` tears down the EGL context,
    /// after which GL calls through a surviving clone are undefined behavior.
    pub fn context_arc(&self) -> Arc<glow::Context> {
        Arc::clone(&self.gl)
    }
}

impl std::ops::Deref for HeadlessGl {
    type Target = glow::Context;

    fn deref(&self) -> &glow::Context {
        &self.gl
    }
}

impl std::ops::DerefMut for HeadlessGl {
    /// `BackdropBlur::record` takes `&mut CommandSink` (`&mut glow::Context`); glow's methods are all
    /// `&self`, so this mutable deref is sound — it only satisfies the seam signature, it does not
    /// imply exclusive GL access beyond the single-threaded harness contract.
    fn deref_mut(&mut self) -> &mut glow::Context {
        // Rule: no outstanding `context_arc` clones may exist while mutably dereferencing the
        // harness. This crate's internal tests never call `context_arc`, so this cannot fire here;
        // a violation elsewhere is a loud, self-diagnosing test bug.
        Arc::get_mut(&mut self.gl)
            .expect("no outstanding context_arc clones while mutating the harness context")
    }
}

impl Drop for HeadlessGl {
    fn drop(&mut self) {
        // Best-effort EGL teardown; never panic in Drop.
        if let Err(e) = self.egl.make_current(self.display, None, None, None) {
            eprintln!("gl_harness: eglMakeCurrent(release) failed: {e:?}");
        }
        if let Err(e) = self.egl.destroy_context(self.display, self.context) {
            eprintln!("gl_harness: eglDestroyContext failed: {e:?}");
        }
        if let Err(e) = self.egl.terminate(self.display) {
            eprintln!("gl_harness: eglTerminate failed: {e:?}");
        }
    }
}

/// Create a surfaceless headless GL context. **Panics** with a diagnostic if EGL/GL cannot be set
/// up: these tests are compile-gated behind `gl-snapshots`, so the runner is expected to have a
/// usable GL stack (IMPL §14 — compile-gated, not runtime-skipped).
pub fn headless_gl() -> HeadlessGl {
    // SAFETY: loading the system EGL library by its standard SONAME; we trust libEGL.so.1 to be a
    // real EGL implementation.
    let lib = unsafe { libloading::Library::new("libEGL.so.1") }.expect("load libEGL.so.1");
    // SAFETY: `lib` is the loaded EGL library; load_required_from reads its exported symbols.
    let egl_instance: Egl = unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required_from(lib) }
        .expect("load EGL >= 1.4 from libEGL.so.1");

    let display = device_platform_display(&egl_instance)
        .or_else(|| default_display(&egl_instance))
        .expect("no usable EGL display (device-platform or default-display)");

    let (config, context) = create_surfaceless_context(&egl_instance, display)
        .expect("could not create a surfaceless desktop-GL/GLES context");
    let _ = config;

    egl_instance
        .make_current(display, None, None, Some(context))
        .expect("eglMakeCurrent(NO_SURFACE, NO_SURFACE) failed — EGL_KHR_surfaceless_context?");

    // SAFETY: an EGL context is current on this thread; glow's loader resolves GL symbols via
    // eglGetProcAddress (NVIDIA GLVND resolves core GL too), and glow reads GL_VERSION immediately,
    // which the current context satisfies.
    let gl = unsafe {
        glow::Context::from_loader_function_cstr(|name: &CStr| {
            match egl_instance.get_proc_address(name.to_string_lossy().as_ref()) {
                Some(f) => f as *const c_void,
                None => std::ptr::null(),
            }
        })
    };

    HeadlessGl {
        egl: egl_instance,
        display,
        context,
        gl: Arc::new(gl),
    }
}

/// The NVIDIA `EGL_EXT_platform_device` path: enumerate devices, get a platform display for the
/// first that initializes.
fn device_platform_display(egl_instance: &Egl) -> Option<egl::Display> {
    let query_ptr = egl_instance.get_proc_address("eglQueryDevicesEXT")?;
    let get_display_ptr = egl_instance.get_proc_address("eglGetPlatformDisplayEXT")?;
    // SAFETY: both pointers came from eglGetProcAddress for exactly these named EXT entry points,
    // whose `extern "system"` ABI matches the spec signatures transmuted here; non-null was checked
    // by get_proc_address returning Some.
    let query_devices: PfnEglQueryDevicesExt = unsafe { std::mem::transmute(query_ptr) };
    // SAFETY: as above, for eglGetPlatformDisplayEXT.
    let get_platform_display: PfnEglGetPlatformDisplayExt =
        unsafe { std::mem::transmute(get_display_ptr) };

    let mut num_devices: egl::Int = 0;
    // SAFETY: spec null-probe — writes only `num_devices`, reads nothing through the null array.
    let ok = unsafe { query_devices(0, std::ptr::null_mut(), &mut num_devices) };
    if ok == egl::FALSE || num_devices <= 0 {
        return None;
    }
    let mut devices: Vec<*mut c_void> = vec![std::ptr::null_mut(); num_devices as usize];
    let mut returned: egl::Int = 0;
    // SAFETY: `devices` has capacity `num_devices`; the call fills at most that many entries.
    let ok = unsafe { query_devices(num_devices, devices.as_mut_ptr(), &mut returned) };
    if ok == egl::FALSE || returned <= 0 {
        return None;
    }

    devices.iter().take(returned as usize).find_map(|&device| {
        if device.is_null() {
            return None;
        }
        // SAFETY: `device` is a valid EGLDeviceEXT from eglQueryDevicesEXT; a null attrib list is
        // permitted. Returns EGL_NO_DISPLAY (null) on failure, checked next.
        let raw =
            unsafe { get_platform_display(EGL_PLATFORM_DEVICE_EXT, device, std::ptr::null()) };
        if raw.is_null() {
            return None;
        }
        // SAFETY: `raw` is a non-null EGLDisplay from the call above.
        let display = unsafe { egl::Display::from_ptr(raw) };
        egl_instance.initialize(display).ok().map(|_| display)
    })
}

/// Fallback: `eglGetDisplay(EGL_DEFAULT_DISPLAY)`.
fn default_display(egl_instance: &Egl) -> Option<egl::Display> {
    // SAFETY: EGL_DEFAULT_DISPLAY is the documented null native-display id.
    let display = unsafe { egl_instance.get_display(egl::DEFAULT_DISPLAY) }?;
    egl_instance.initialize(display).ok().map(|_| display)
}

/// Choose an RGBA8 PBUFFER config and create a surfaceless context — desktop GL 3.3 (no profile
/// mask), falling back to GLES 3.0.
fn create_surfaceless_context(
    egl_instance: &Egl,
    display: egl::Display,
) -> Option<(egl::Config, egl::Context)> {
    #[rustfmt::skip]
    let gl_config_attribs = [
        egl::SURFACE_TYPE, egl::PBUFFER_BIT,
        egl::RENDERABLE_TYPE, egl::OPENGL_BIT,
        egl::RED_SIZE, 8, egl::GREEN_SIZE, 8, egl::BLUE_SIZE, 8, egl::ALPHA_SIZE, 8,
        egl::NONE,
    ];
    if egl_instance.bind_api(egl::OPENGL_API).is_ok()
        && let Ok(Some(config)) = egl_instance.choose_first_config(display, &gl_config_attribs)
    {
        // MAJOR/MINOR only — the profile mask attrib is rejected by this NVIDIA libEGL.
        let ctx_attribs = [
            EGL_CONTEXT_MAJOR_VERSION,
            3,
            EGL_CONTEXT_MINOR_VERSION,
            3,
            egl::NONE,
        ];
        if let Ok(ctx) = egl_instance.create_context(display, config, None, &ctx_attribs) {
            return Some((config, ctx));
        }
        // Driver-max desktop context (no version attribs).
        if let Ok(ctx) = egl_instance.create_context(display, config, None, &[egl::NONE]) {
            return Some((config, ctx));
        }
    }

    #[rustfmt::skip]
    let es_config_attribs = [
        egl::SURFACE_TYPE, egl::PBUFFER_BIT,
        egl::RENDERABLE_TYPE, egl::OPENGL_ES3_BIT,
        egl::RED_SIZE, 8, egl::GREEN_SIZE, 8, egl::BLUE_SIZE, 8, egl::ALPHA_SIZE, 8,
        egl::NONE,
    ];
    if egl_instance.bind_api(egl::OPENGL_ES_API).is_ok()
        && let Ok(Some(config)) = egl_instance.choose_first_config(display, &es_config_attribs)
    {
        let ctx_attribs = [
            EGL_CONTEXT_MAJOR_VERSION,
            3,
            EGL_CONTEXT_MINOR_VERSION,
            0,
            egl::NONE,
        ];
        if let Ok(ctx) = egl_instance.create_context(display, config, None, &ctx_attribs) {
            return Some((config, ctx));
        }
    }
    None
}

/// Read a single RGBA8 pixel at `(x, y)` from the current draw framebuffer. Calls `glFinish` first.
pub fn read_rgba8(gl: &glow::Context, x: i32, y: i32) -> [u8; 4] {
    let mut px = [0_u8; 4];
    // SAFETY: a GL context is current; `read_pixels` writes exactly 4 bytes (1 RGBA8 pixel) into
    // `px`, whose length matches the 1x1 RGBA/UNSIGNED_BYTE request.
    unsafe {
        gl.finish();
        gl.read_pixels(
            x,
            y,
            1,
            1,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut px)),
        );
    }
    px
}

/// Read a single RGBA8 pixel at `(x, y)` from a `texture` (bottom-left origin), by attaching it to a
/// throwaway read framebuffer. Restores no state — call only on the harness context where the test
/// owns all bindings.
pub fn read_texture_rgba8(gl: &glow::Context, texture: glow::Texture, x: i32, y: i32) -> [u8; 4] {
    let mut px = [0_u8; 4];
    // SAFETY: a GL context is current; a throwaway FBO is created, `texture` attached as color 0,
    // one pixel read, then both the FBO binding cleared and the FBO deleted. `texture` is a live
    // handle the caller owns.
    unsafe {
        let fbo = gl.create_framebuffer().expect("readback fbo");
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::READ_FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        gl.finish();
        gl.read_pixels(
            x,
            y,
            1,
            1,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut px)),
        );
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
        gl.delete_framebuffer(fbo);
    }
    px
}
