//! Mastertech screenshot WASM plugin. Shells PowerShell captures, returns base64 PNG.

const BUF: usize = 8 * 1024 * 1024;
static mut HEAP: [u8; BUF] = [0; BUF];
static mut HEAP_POS: usize = 0;

const OUT_CAP: i32 = 3 * 1024 * 1024;

unsafe extern "C" {
    fn host_run_command(cmd_ptr: i32, cmd_len: i32, out_ptr: i32, out_max: i32) -> i32;
}

fn align_up(pos: usize, align: usize) -> usize {
    (pos + align - 1) & !(align - 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(size: i32) -> i32 {
    unsafe {
        let size = size as usize;
        if size == 0 {
            return 1;
        }
        let p = align_up(HEAP_POS, 16);
        if p + size > BUF {
            return 0;
        }
        HEAP_POS = p + size;
        (&raw mut HEAP).cast::<u8>().add(p) as i32
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn dealloc(_ptr: i32, _size: i32) {}

fn leak_bytes(slice: &[u8]) -> u64 {
    unsafe {
        let len = slice.len() as i32;
        let ptr = alloc(len);
        if ptr == 0 {
            return 0;
        }
        std::ptr::copy_nonoverlapping(slice.as_ptr(), ptr as *mut u8, slice.len());
        ((ptr as u64) << 32) | (len as u64 & 0xffff_ffff)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn plugin_id() -> u64 {
    leak_bytes(b"com.mastertech.screenshot")
}

#[unsafe(no_mangle)]
pub extern "C" fn plugin_name() -> u64 {
    leak_bytes(b"Screenshot Capture")
}

#[unsafe(no_mangle)]
pub extern "C" fn plugin_version() -> u64 {
    leak_bytes(b"0.1.0")
}

#[unsafe(no_mangle)]
pub extern "C" fn on_load() {}

#[unsafe(no_mangle)]
pub extern "C" fn on_unload() {}

#[unsafe(no_mangle)]
pub extern "C" fn logic() {}

#[unsafe(no_mangle)]
pub extern "C" fn ui_commands() -> u64 {
    leak_bytes(b"[]")
}

#[unsafe(no_mangle)]
pub extern "C" fn mcp_tools() -> u64 {
    leak_bytes(
        br#"[
{"name":"capture_hyperv_vm","description":"Capture a Hyper-V VM console as a PNG via WMI GetVirtualSystemThumbnailImage. Works without guest Integration Components. Args: vm_name (required), width (default 320), height (default 240).","parameters_schema":{"type":"object","properties":{"vm_name":{"type":"string"},"width":{"type":"integer"},"height":{"type":"integer"}},"required":["vm_name"]}},
{"name":"capture_window","description":"Capture the first top-level window whose title contains the given substring, as a PNG via PrintWindow (captures unfocused/background windows). Args: title (required).","parameters_schema":{"type":"object","properties":{"title":{"type":"string"}},"required":["title"]}},
{"name":"capture_desktop","description":"Capture the full virtual desktop, or a single monitor, as a PNG via CopyFromScreen. Args: monitor (optional 0-based index).","parameters_schema":{"type":"object","properties":{"monitor":{"type":"integer"}}}}
]"#,
    )
}

// Runs PowerShell via the host and returns trimmed stdout.
fn run(cmd: &str) -> String {
    let out_ptr = alloc(OUT_CAP);
    if out_ptr == 0 {
        return String::from("[error] out buffer alloc failed");
    }
    let n = unsafe { host_run_command(cmd.as_ptr() as i32, cmd.len() as i32, out_ptr, OUT_CAP) };
    let n = n.max(0) as usize;
    unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(out_ptr as *const u8, n))
            .unwrap_or("[error] non-utf8 output")
            .trim()
            .to_string()
    }
}

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

fn ps_hyperv(vm: &str, w: i64, h: i64) -> String {
    HYPERV_TPL
        .replace("{VM}", &ps_quote(vm))
        .replace("{W}", &w.to_string())
        .replace("{H}", &h.to_string())
}

fn ps_window(title: &str) -> String {
    WINDOW_TPL.replace("{TITLE}", &ps_quote(title))
}

fn ps_desktop(monitor: Option<i64>) -> String {
    let bounds = match monitor {
        Some(i) => format!(
            "$s=[System.Windows.Forms.Screen]::AllScreens[{}].Bounds;",
            i.max(0)
        ),
        None => "$s=[System.Windows.Forms.SystemInformation]::VirtualScreen;".to_string(),
    };
    DESKTOP_TPL.replace("{BOUNDS}", &bounds)
}

const HYPERV_TPL: &str = r#"$ErrorActionPreference='Stop';Add-Type -AssemblyName System.Drawing;$ns='root\virtualization\v2';$vm=Get-CimInstance -Namespace $ns -ClassName Msvm_ComputerSystem -Filter "ElementName='{VM}' AND Caption='Virtual Machine'";if(-not $vm){throw 'vm not found'};$settings=Get-CimAssociatedInstance -InputObject $vm -Association Msvm_SettingsDefineState -ResultClassName Msvm_VirtualSystemSettingData;$svc=Get-CimInstance -Namespace $ns -ClassName Msvm_VirtualSystemManagementService;$r=Invoke-CimMethod -InputObject $svc -MethodName GetVirtualSystemThumbnailImage -Arguments @{WidthPixels=[uint16]{W};HeightPixels=[uint16]{H};TargetSystem=$settings};if($r.ReturnValue -ne 0){throw "thumbnail failed $($r.ReturnValue)"};$img=$r.ImageData;if(-not $img){throw 'no image data'};$bmp=New-Object System.Drawing.Bitmap({W},{H},[System.Drawing.Imaging.PixelFormat]::Format16bppRgb565);$rect=New-Object System.Drawing.Rectangle(0,0,{W},{H});$bd=$bmp.LockBits($rect,[System.Drawing.Imaging.ImageLockMode]::WriteOnly,[System.Drawing.Imaging.PixelFormat]::Format16bppRgb565);[System.Runtime.InteropServices.Marshal]::Copy($img,0,$bd.Scan0,$img.Length);$bmp.UnlockBits($bd);$ms=New-Object System.IO.MemoryStream;$bmp.Save($ms,[System.Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($ms.ToArray())"#;

const WINDOW_TPL: &str = r#"$ErrorActionPreference='Stop';Add-Type -AssemblyName System.Drawing;Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public class Win {
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr d,uint f);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 public struct RECT { public int L; public int T; public int R; public int B; }
}
'@
$p=Get-Process|Where-Object {$_.MainWindowTitle -like '*{TITLE}*' -and $_.MainWindowHandle -ne 0}|Select-Object -First 1;if(-not $p){throw 'window not found'};$h=$p.MainWindowHandle;$r=New-Object Win+RECT;[Win]::GetWindowRect($h,[ref]$r)|Out-Null;$w=$r.R-$r.L;$ht=$r.B-$r.T;if($w -le 0 -or $ht -le 0){throw 'bad window rect'};$bmp=New-Object System.Drawing.Bitmap($w,$ht);$g=[System.Drawing.Graphics]::FromImage($bmp);$hdc=$g.GetHdc();[Win]::PrintWindow($h,$hdc,2)|Out-Null;$g.ReleaseHdc($hdc);$ms=New-Object System.IO.MemoryStream;$bmp.Save($ms,[System.Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($ms.ToArray())"#;

const DESKTOP_TPL: &str = r#"$ErrorActionPreference='Stop';Add-Type -AssemblyName System.Drawing;Add-Type -AssemblyName System.Windows.Forms;{BOUNDS}$bmp=New-Object System.Drawing.Bitmap($s.Width,$s.Height);$g=[System.Drawing.Graphics]::FromImage($bmp);$g.CopyFromScreen($s.X,$s.Y,0,0,$bmp.Size);$ms=New-Object System.IO.MemoryStream;$bmp.Save($ms,[System.Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($ms.ToArray())"#;

fn arg_str(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn arg_i64(v: &serde_json::Value, k: &str, d: i64) -> i64 {
    v.get(k).and_then(|x| x.as_i64()).unwrap_or(d)
}

// Wraps a base64 PNG in the image envelope the MCP bridge turns into an image content block.
fn envelope(b64: &str) -> u64 {
    if b64.is_empty() || b64.contains("[stderr]") || b64.contains("[error]") {
        let msg = if b64.is_empty() { "empty capture output" } else { b64 };
        return leak_bytes(serde_json::json!({ "error": msg }).to_string().as_bytes());
    }
    let result = serde_json::json!({ "image_base64": b64, "mime": "image/png" });
    leak_bytes(result.to_string().as_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn handle_mcp_call(
    tool_ptr: i32,
    tool_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> u64 {
    if tool_len <= 0 || tool_ptr <= 0 {
        return leak_bytes(br#"{"error":"bad tool"}"#);
    }
    let tool = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(tool_ptr as *const u8, tool_len as usize))
            .unwrap_or("")
    };
    let args_str = unsafe {
        if args_len > 0 && args_ptr > 0 {
            std::str::from_utf8(std::slice::from_raw_parts(args_ptr as *const u8, args_len as usize))
                .unwrap_or("{}")
        } else {
            "{}"
        }
    };
    let args: serde_json::Value = serde_json::from_str(args_str).unwrap_or(serde_json::Value::Null);

    let packed = match tool {
        "capture_hyperv_vm" => {
            let vm = arg_str(&args, "vm_name");
            if vm.is_empty() {
                leak_bytes(br#"{"error":"vm_name required"}"#)
            } else {
                let w = arg_i64(&args, "width", 320);
                let h = arg_i64(&args, "height", 240);
                envelope(&run(&ps_hyperv(&vm, w, h)))
            }
        }
        "capture_window" => {
            let title = arg_str(&args, "title");
            if title.is_empty() {
                leak_bytes(br#"{"error":"title required"}"#)
            } else {
                envelope(&run(&ps_window(&title)))
            }
        }
        "capture_desktop" => {
            let mon = args.get("monitor").and_then(|x| x.as_i64());
            envelope(&run(&ps_desktop(mon)))
        }
        _ => leak_bytes(br#"{"error":"unknown tool"}"#),
    };

    unsafe {
        HEAP_POS = 0;
    }
    packed
}
