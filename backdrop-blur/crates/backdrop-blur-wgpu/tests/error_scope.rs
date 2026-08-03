//! Gated GPU tier: the executed **proxy** for the wgpu backend's private `scoped_oom` capture
//! machinery (design: issue `own-loop-oom-capture-executed-test`). A real device out-of-memory has
//! no proportionate injection vehicle on this stack, so rather than forcing one, this drives the
//! same wgpu error-scope primitive `scoped_oom` rides — push a filtered scope, create a resource,
//! poll the pop once — using the *validation* error class, which lavapipe produces
//! deterministically. It pins the three properties `scoped_oom` depends on:
//!
//! 1. a matching-class error is captured, synchronously (one poll of `pop()` is `Ready`);
//! 2. a scope pops empty on success (what `scoped_oom` reads as `Ok(resource)`);
//! 3. an out-of-memory scope does **not** catch a validation error — it passes through to an
//!    enclosing scope — so the filter is selective and will not swallow a non-out-of-memory fault.
//!
//! This is a proxy at one remove: it does not call the private `scoped_oom`/`poll_once`, and does
//! not force a real out-of-memory. Runs only with `--features image-snapshots` on a host with a
//! Vulkan software rasterizer (lavapipe).
#![cfg(feature = "image-snapshots")]

mod common;

use common::software_device;

/// Poll a future exactly once and return its output, asserting it is already ready — the native
/// error-scope contract the crate's private `poll_once` relies on: `pop()` resolves synchronously
/// because the fault was recorded during the create call. Panics on `Pending` (the non-native
/// deferred path this crate does not support); deliberately **not** `pollster::block_on`, which
/// would tolerate a `Pending` and prove a weaker property than the code depends on.
fn poll_once_ready<F: std::future::Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    match fut.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => v,
        std::task::Poll::Pending => {
            panic!("error-scope pop must resolve synchronously on the native path")
        }
    }
}

/// A buffer descriptor one alignment unit past the device's `max_buffer_size`. wgpu-core rejects it
/// with a *validation*-class error via a pure size comparison before any allocation, so it is a
/// deterministic, allocation-free way to raise a known error class. The returned handle is invalid
/// and must never be mapped or used.
fn over_limit_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("over-limit"),
        size: device.limits().max_buffer_size + wgpu::COPY_BUFFER_ALIGNMENT,
        usage: wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// A matching-class error is captured, synchronously: a validation-filtered scope around a
/// deterministic over-limit allocation yields the validation error from a single poll of `pop()`.
#[test]
fn validation_scope_captures_validation_error_synchronously() {
    let (device, _queue) = software_device();
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _invalid = over_limit_buffer(&device);
    let captured = poll_once_ready(scope.pop());
    assert!(
        matches!(captured, Some(wgpu::Error::Validation { .. })),
        "a validation-filtered scope must capture the over-limit buffer's validation error, got {captured:?}"
    );
}

/// The success arm: an out-of-memory scope around a valid allocation pops empty, synchronously —
/// exactly what `scoped_oom` reads as `Ok(resource)`.
#[test]
fn out_of_memory_scope_pops_empty_on_success_synchronously() {
    let (device, _queue) = software_device();
    let scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let _ok = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("valid"),
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let captured = poll_once_ready(scope.pop());
    assert!(
        captured.is_none(),
        "an out-of-memory scope must pop empty when the allocation succeeds, got {captured:?}"
    );
}

/// Selectivity — the load-bearing property behind `scoped_oom`: an out-of-memory-filtered scope
/// does not capture a validation error, but lets it pass through to the enclosing validation scope.
/// So `scoped_oom`'s out-of-memory scope will not swallow a non-out-of-memory fault.
#[test]
fn out_of_memory_scope_passes_non_matching_class_through() {
    let (device, _queue) = software_device();
    // Enclosing scope catches validation; inner scope filters out-of-memory only.
    let outer = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let inner = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let _invalid = over_limit_buffer(&device);

    // The inner out-of-memory scope must NOT capture the validation error (pop it first: LIFO)...
    let inner_pop = poll_once_ready(inner.pop());
    assert!(
        inner_pop.is_none(),
        "an out-of-memory scope must not capture a validation error, got {inner_pop:?}"
    );
    // ...it passes through to the enclosing validation scope.
    let outer_pop = poll_once_ready(outer.pop());
    assert!(
        matches!(outer_pop, Some(wgpu::Error::Validation { .. })),
        "the validation error must pass through to the enclosing validation scope, got {outer_pop:?}"
    );
}
