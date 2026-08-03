//! Shared helpers for the gated GPU tier (`--features image-snapshots`). This file lives under
//! `tests/common/`, so Cargo never compiles it as its own test binary; it is pulled in via
//! `mod common;` from each gated test file and therefore inherits that file's feature gate.

/// A headless device on the software adapter (lavapipe), so the test is deterministic and
/// needs no real GPU.
pub fn software_device() -> (wgpu::Device, wgpu::Queue) {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    // The gated tier pins VK_ICD_FILENAMES to lavapipe, so the only Vulkan adapter the loader
    // exposes is the software rasterizer; no `force_fallback_adapter` filtering is needed (and it
    // would wrongly exclude lavapipe, which Vulkan does not flag as a DX12-WARP-style fallback).
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("a Vulkan adapter (lavapipe via VK_ICD_FILENAMES) is required for the gated GPU tier");

    // Use the adapter's own limits so the request can never exceed what a software backend
    // (lavapipe Vulkan, or Mesa llvmpipe GL) actually provides.
    let limits = adapter.limits();
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("backdrop-blur test device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("device creation on the software adapter")
}
