//! # mtech-plugin-sdk
//!
//! Authoring surface for Mastertech WASM guest plugins. Guests parse and
//! serialize with `serde_json` over the existing JSON ABI; `#[derive(Facet)]`
//! supplies static SHAPE data for two things only: generating each tool's
//! `parameters_schema` JSON and a structural fingerprint.
//!
//! ## Authoring
//!
//! ```ignore
//! use mtech_plugin_sdk::{mtech_plugin, host, SdkError};
//! use facet::Facet;
//! use serde::Deserialize;
//!
//! #[derive(Facet, Deserialize)]
//! struct ExportArgs {
//!     /// Published INF name, e.g. oem12.inf.
//!     published_name: String,
//! }
//!
//! fn export(a: ExportArgs) -> Result<serde_json::Value, SdkError> {
//!     let out = host::run_command(&format!("pnputil /export-driver {}", a.published_name));
//!     Ok(serde_json::json!({ "tool": "export_driver", "data": out }))
//! }
//!
//! mtech_plugin! {
//!     id: "com.example.demo",
//!     name: "Demo",
//!     version: "0.1.0",
//!     tools: {
//!         /// Export one driver package.
//!         export_driver(ExportArgs) => export,
//!     }
//! }
//! ```
//!
//! ### Handler contract
//! - With args: `fn(Args) -> Result<T, SdkError>` where
//!   `Args: facet::Facet<'static> + serde::de::DeserializeOwned` and `T: serde::Serialize`.
//! - No-arg: `fn() -> Result<T, SdkError>`.
//! - The success value is serialized at top level, unwrapped.
//!
//! ### Heap knob
//! `heap: <bytes>` sizes the ABI bump arena (default 1 MiB). Text tools fit in
//! the default; screenshot-class plugins set `heap: 8 * 1024 * 1024`. Overflow
//! is loud: `alloc` returns `-1` and `emit` returns a const overflow envelope.
//!
//! ### Rename rule
//! Do not rename fields. If unavoidable, apply BOTH `#[facet(rename = "...")]`
//! and `#[serde(rename = "...")]`: the schema and fingerprint key off the facet
//! rename, deserialization off the serde rename.
//!
//! ### Panic rule
//! Handlers return `SdkError`, they do not panic. A panic hook logs the message
//! and location to the host before the guest traps; there is no `catch_unwind`
//! on stable wasm32-wasip1.

pub mod dispatch;
pub mod error;
pub mod host;
pub mod marshal;
pub mod schema;

pub use error::{ErrorCode, SdkError};
pub use schema::{ToolDef, ToolSet};

pub use facet;
pub use serde_json;

/// SDK ABI contract version; bump when the host import/export contract changes.
pub const ABI_VERSION: u32 = 1;

/// Macro plumbing referenced by `mtech_plugin!` expansion; not a stable API.
#[doc(hidden)]
pub mod __rt {
    pub use crate::dispatch::{
        install_panic_hook, lenient_args, normalize_doc, ok_json, parse_args, run_tool,
    };
    #[cfg(target_arch = "wasm32")]
    pub use crate::marshal::{arena_alloc, arena_init, arena_reset, emit, read_input};
}

/// Declarative plugin definition: emits the full WASM ABI export set.
#[macro_export]
macro_rules! mtech_plugin {
    (
        id: $id:literal,
        name: $name:literal,
        version: $ver:literal,
        $(heap: $heap:expr,)?
        $(on_load: $on_load:path,)?
        $(ui_commands: $ui:path,)?
        tools: {
            $(
                $(#[doc = $doc:literal])+
                $tool:ident ( $($args:ty)? ) => $handler:path
            ),* $(,)?
        }
    ) => {
        fn __mtech_tools() -> $crate::ToolSet {
            let mut __t = $crate::ToolSet::new();
            $(
                __t.push(
                    ::core::stringify!($tool),
                    ::core::concat!($($doc),+),
                    $crate::__arg_shape!($($args)?),
                );
            )*
            __t
        }

        fn __mtech_dispatch(__tool: &str, __args: $crate::serde_json::Value) -> ::std::string::String {
            match __tool {
                $(
                    ::core::stringify!($tool) => $crate::__rt::run_tool(
                        ::core::stringify!($tool),
                        || $crate::__call_handler!(::core::stringify!($tool), $handler, __args $(, $args)?),
                    ),
                )*
                __other => $crate::SdkError::not_found("unknown tool").with_tool(__other).to_json(),
            }
        }

        #[allow(dead_code)]
        fn __mtech_on_load() {
            $crate::__rt::install_panic_hook();
            $( $on_load(); )?
            $crate::host::log(::core::concat!($name, " v", $ver, " loaded"));
        }

        #[allow(dead_code)]
        fn __mtech_ui() -> ::std::string::String {
            $crate::__ui_body!($($ui)?)
        }

        #[cfg(target_arch = "wasm32")]
        mod __mtech_abi {
            $crate::__arena_decl!($($heap)?);

            #[unsafe(no_mangle)]
            pub extern "C" fn alloc(n: i32) -> i32 {
                __mtech_arena_init();
                $crate::__rt::arena_alloc(n)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn dealloc(_p: i32, _n: i32) {}

            #[unsafe(no_mangle)]
            pub extern "C" fn plugin_id() -> u64 {
                __mtech_arena_init();
                $crate::__rt::arena_reset();
                $crate::__rt::emit($id)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn plugin_name() -> u64 {
                __mtech_arena_init();
                $crate::__rt::arena_reset();
                $crate::__rt::emit($name)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn plugin_version() -> u64 {
                __mtech_arena_init();
                $crate::__rt::arena_reset();
                $crate::__rt::emit($ver)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn plugin_abi_version() -> u32 {
                $crate::ABI_VERSION
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn plugin_fingerprint() -> u64 {
                super::__mtech_tools().fingerprint()
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn on_load() {
                super::__mtech_on_load();
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn on_unload() {}

            #[unsafe(no_mangle)]
            pub extern "C" fn logic() {}

            #[unsafe(no_mangle)]
            pub extern "C" fn ui_commands() -> u64 {
                __mtech_arena_init();
                $crate::__rt::arena_reset();
                $crate::__rt::emit(&super::__mtech_ui())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn mcp_tools() -> u64 {
                __mtech_arena_init();
                $crate::__rt::arena_reset();
                $crate::__rt::emit(&super::__mtech_tools().to_tools_json())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn handle_mcp_call(tp: i32, tl: i32, ap: i32, al: i32) -> u64 {
                __mtech_arena_init();
                let (__tool, __args) = $crate::__rt::read_input(tp, tl, ap, al);
                $crate::__rt::emit(&super::__mtech_dispatch(&__tool, __args))
            }
        }
    };
}

/// `Some(SHAPE)` for a typed arg, `None` for a no-arg tool.
#[doc(hidden)]
#[macro_export]
macro_rules! __arg_shape {
    () => {
        ::core::option::Option::None
    };
    ($t:ty) => {
        ::core::option::Option::Some(<$t as $crate::facet::Facet<'static>>::SHAPE)
    };
}

/// Parses args (if any) and calls the handler, returning serialized JSON.
#[doc(hidden)]
#[macro_export]
macro_rules! __call_handler {
    ($tool:expr, $handler:path, $args:ident) => {{
        let _ = $args;
        ::core::result::Result::Ok($crate::__rt::ok_json(&$handler()?))
    }};
    ($tool:expr, $handler:path, $args:ident, $t:ty) => {{
        let __a: $t = $crate::__rt::parse_args::<$t>($tool, $args)?;
        ::core::result::Result::Ok($crate::__rt::ok_json(&$handler(__a)?))
    }};
}

/// Declares the arena backing buffer and its idempotent initializer.
#[doc(hidden)]
#[macro_export]
macro_rules! __arena_decl {
    () => {
        static mut __MTECH_HEAP: [u8; 1024 * 1024] = [0u8; 1024 * 1024];
        #[inline]
        fn __mtech_arena_init() {
            $crate::__rt::arena_init(&raw mut __MTECH_HEAP as *mut u8, 1024 * 1024);
        }
    };
    ($heap:expr) => {
        static mut __MTECH_HEAP: [u8; $heap] = [0u8; $heap];
        #[inline]
        fn __mtech_arena_init() {
            $crate::__rt::arena_init(&raw mut __MTECH_HEAP as *mut u8, $heap);
        }
    };
}

/// The `ui_commands` body: user function or the empty `"[]"` default.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_body {
    () => {
        ::std::string::String::from("[]")
    };
    ($ui:path) => {
        $ui()
    };
}
