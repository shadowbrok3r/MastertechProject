;; Mastertech clock WASM guest — placeholders __ID_OFF__ __ID_LEN__ __NAME_OFF__ __NAME_LEN__
;; __VER_OFF__ __VER_LEN__ __TOOLS_OFF__ __TOOLS_LEN__ __TOOL_CMP_OFF__ __ERR_OFF__ __ERR_LEN__ __UI_OFF__
(module
  (import "env" "host_log" (func $host_log (param i32 i32)))
  (import "env" "host_emit_event" (func $host_emit_event (param i32 i32)))
  (import "env" "host_repaint" (func $host_repaint))
  (import "env" "host_fill_clock_json" (func $host_fill_clock_json (param i32 i32) (result i32)))

  (memory (export "memory") 1)
  (global $g_hp (mut i32) (i32.const 2048))

  ;; Binary payload: wat crate requires a quoted data string (\hh escapes).
  (data (i32.const 1024) "___DATA___")

  (func $alloc (export "alloc") (param $n i32) (result i32)
    (local $p i32)
    (local $a i32)
    (local.set $p (global.get $g_hp))
    (local.set $a (i32.add (local.get $n) (i32.const 15)))
    (local.set $a (i32.and (local.get $a) (i32.const -16)))
    (global.set $g_hp (i32.add (global.get $g_hp) (local.get $a)))
    (local.get $p))

  (func $dealloc (export "dealloc") (param i32 i32))

  (func (export "plugin_id") (result i64)
    (i64.or
      (i64.shl (i64.extend_i32_u (i32.const __ID_OFF__)) (i64.const 32))
      (i64.extend_i32_u (i32.const __ID_LEN__))))

  (func (export "plugin_name") (result i64)
    (i64.or
      (i64.shl (i64.extend_i32_u (i32.const __NAME_OFF__)) (i64.const 32))
      (i64.extend_i32_u (i32.const __NAME_LEN__))))

  (func (export "plugin_version") (result i64)
    (i64.or
      (i64.shl (i64.extend_i32_u (i32.const __VER_OFF__)) (i64.const 32))
      (i64.extend_i32_u (i32.const __VER_LEN__))))

  (func (export "on_load"))
  (func (export "on_unload"))
  (func (export "logic"))

  (func (export "ui_commands") (result i64)
    (i64.or
      (i64.shl (i64.extend_i32_u (i32.const __UI_OFF__)) (i64.const 32))
      (i64.extend_i32_u (i32.const 2))))

  (func (export "mcp_tools") (result i64)
    (i64.or
      (i64.shl (i64.extend_i32_u (i32.const __TOOLS_OFF__)) (i64.const 32))
      (i64.extend_i32_u (i32.const __TOOLS_LEN__))))

  ;; handle_mcp_call: if tool == "current_time", host fills JSON; else return error JSON
  (func (export "handle_mcp_call") (param $tp i32) (param $tl i32) (param $ap i32) (param $al i32) (result i64)
    (local $i i32)
    (local $buf i32)
    (local $n i32)
    (if (i32.ne (local.get $tl) (i32.const 12))
      (then
        (return
          (i64.or
            (i64.shl (i64.extend_i32_u (i32.const __ERR_OFF__)) (i64.const 32))
            (i64.extend_i32_u (i32.const __ERR_LEN__))))))
    (local.set $i (i32.const 0))
    (loop $cmp
      (if (i32.eq (local.get $i) (i32.const 12))
        (then
          (local.set $buf (call $alloc (i32.const 256)))
          (if (i32.eqz (local.get $buf))
            (then
              (return
                (i64.or
                  (i64.shl (i64.extend_i32_u (i32.const __ERR_OFF__)) (i64.const 32))
                  (i64.extend_i32_u (i32.const __ERR_LEN__))))))
          (local.set $n (call $host_fill_clock_json (local.get $buf) (i32.const 256)))
          (return
            (i64.or
              (i64.shl (i64.extend_i32_u (local.get $buf)) (i64.const 32))
              (i64.extend_i32_u (local.get $n))))))
      (if (i32.ne
            (i32.load8_u (i32.add (local.get $tp) (local.get $i)))
            (i32.load8_u (i32.add (i32.const __TOOL_CMP_OFF__) (local.get $i))))
        (then
          (return
            (i64.or
              (i64.shl (i64.extend_i32_u (i32.const __ERR_OFF__)) (i64.const 32))
              (i64.extend_i32_u (i32.const __ERR_LEN__))))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $cmp))
    (unreachable))
)
