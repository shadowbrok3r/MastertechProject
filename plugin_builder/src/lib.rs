//! Wire types for the `plugin_builder` worker.
//!
//! The worker connects to `websocket_server2` as `role=client` in a room
//! named `build_worker_<hostname>`, then talks to the admin/MCP side
//! using bincode-serialized [`BuilderWire`] messages. The big
//! `displays::Cmd` enum is **not** used here on purpose — pulling it
//! in would drag eframe, surrealdb, async-openai, and the entire
//! desktop dep tree into the worker's Docker image.
//!
//! Slice 1 only exercises Hello / CompileRequest / CompileResult; the
//! admin-side translator (slice 2) will adapt these to the new
//! `Cmd::BuildWorkerHello` / `Cmd::CompilePluginRequest` / etc.
//! variants so the rest of the existing admin transport keeps
//! a single homogeneous enum surface.

pub mod wire;
pub mod compile;
pub mod db_mode;

pub use wire::{BuilderWire, CompileProfile, CompileTarget, BUILDER_WIRE_TAG};
pub use compile::{compile_one, BuildArtifact, BuildFailure, Config};
