//! ABI-boundary marshalling: packed pointers and the per-plugin bump arena.

/// Packs a guest pointer and length into `ptr<<32 | len`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) const fn pack(ptr: i32, len: i32) -> u64 {
    ((ptr as u32 as u64) << 32) | (len as u32 as u64)
}

#[cfg(target_arch = "wasm32")]
mod arena {
    use super::pack;

    struct Arena {
        heap: *mut u8,
        cap: usize,
        pos: usize,
    }

    // Single-threaded wasm guest; the host serializes every export call.
    static mut ARENA: Arena = Arena { heap: core::ptr::null_mut(), cap: 0, pos: 0 };

    const OOM_JSON: &str = r#"{"error":"plugin arena overflow; raise heap:","error_code":"internal"}"#;

    const fn align16(pos: usize) -> usize {
        (pos + 15) & !15
    }

    /// Registers the guest-owned backing buffer once.
    pub fn arena_init(heap: *mut u8, cap: usize) {
        let a = &raw mut ARENA;
        unsafe {
            if (*a).heap.is_null() {
                (*a).heap = heap;
                (*a).cap = cap;
                (*a).pos = 0;
            }
        }
    }

    pub fn arena_reset() {
        let a = &raw mut ARENA;
        unsafe {
            (*a).pos = 0;
        }
    }

    /// 16-byte-aligned bump; returns `-1` on overflow.
    pub fn arena_alloc(n: i32) -> i32 {
        let a = &raw mut ARENA;
        unsafe {
            let cur = align16((*a).pos);
            let need = cur + (n.max(0) as usize);
            if need > (*a).cap {
                return -1;
            }
            (*a).pos = need;
            ((*a).heap as usize + cur) as i32
        }
    }

    /// Copies `s` into the arena and packs its pointer/length; OOM returns the const error JSON.
    pub fn emit(s: &str) -> u64 {
        let a = &raw mut ARENA;
        let bytes = s.as_bytes();
        unsafe {
            let cur = align16((*a).pos);
            let need = cur + bytes.len();
            if need > (*a).cap {
                return pack(OOM_JSON.as_ptr() as i32, OOM_JSON.len() as i32);
            }
            let dst = ((*a).heap as usize + cur) as *mut u8;
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            (*a).pos = need;
            pack(dst as i32, bytes.len() as i32)
        }
    }

    /// Copies tool + args out to owned values, then resets the arena.
    pub fn read_input(tp: i32, tl: i32, ap: i32, al: i32) -> (String, serde_json::Value) {
        let tool = read_str(tp, tl);
        let args = if al > 0 {
            match serde_json::from_str::<serde_json::Value>(&read_str(ap, al)) {
                Ok(v) => v,
                Err(_) => serde_json::Value::Null,
            }
        } else {
            serde_json::Value::Null
        };
        arena_reset();
        (tool, args)
    }

    fn read_str(ptr: i32, len: i32) -> String {
        if len <= 0 {
            return String::new();
        }
        let slice = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
        String::from_utf8_lossy(slice).into_owned()
    }
}

#[cfg(target_arch = "wasm32")]
pub use arena::{arena_alloc, arena_init, arena_reset, emit, read_input};

#[cfg(test)]
mod tests {
    use super::pack;

    #[test]
    fn pack_roundtrip() {
        let packed = pack(0x1234, 0x56);
        assert_eq!((packed >> 32) as i32, 0x1234);
        assert_eq!((packed & 0xFFFF_FFFF) as i32, 0x56);
    }
}
