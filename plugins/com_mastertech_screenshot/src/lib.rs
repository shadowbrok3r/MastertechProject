//! Screenshot capture plugin (SDK port).
//!
//! Captures a Hyper-V VM console, a background window (PrintWindow), or the
//! desktop, returning a base64 PNG the MCP bridge renders inline. Uses the
//! host's structured command runner so an oversized capture is reported as an
//! error rather than a silently truncated (corrupt) image, and makes the
//! process DPI-aware so scaled displays are not cropped.

use facet::Facet;
use mtech_plugin_sdk::{host, mtech_plugin, SdkError};
use serde::Deserialize;

/// Cap on the base64 envelope; a ~4K PNG base64-encodes to a few MB.
const CAP: usize = 12 * 1024 * 1024;
const TIMEOUT_MS: u64 = 30_000;

#[derive(Facet, Deserialize)]
struct HyperVArgs {
    /// VM ElementName.
    vm_name: String,
    /// Thumbnail width (default 320).
    width: Option<u32>,
    /// Thumbnail height (default 240).
    height: Option<u32>,
}

#[derive(Facet, Deserialize)]
struct WindowArgs {
    /// Substring of the target window title.
    title: String,
}

#[derive(Facet, Deserialize)]
struct DesktopArgs {
    /// 0-based monitor index; omit for the whole virtual desktop.
    monitor: Option<u32>,
}

const DPI_PRELUDE: &str = "Add-Type @'\nusing System;using System.Runtime.InteropServices;\npublic class Dpi { [DllImport(\"user32.dll\")] public static extern bool SetProcessDPIAware(); }\n'@\n[Dpi]::SetProcessDPIAware() | Out-Null;";

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Runs a capture command and returns the image object or an error object.
fn capture(cmd: &str) -> serde_json::Value {
    let out = host::run_command_v2(cmd, TIMEOUT_MS, CAP);
    let b64 = out.stdout.trim();
    if out.timed_out {
        return serde_json::json!({ "error": "capture timed out" });
    }
    if out.truncated {
        return serde_json::json!({ "error": "capture exceeded the size cap; image would be truncated" });
    }
    if b64.is_empty() {
        let msg = if out.stderr.trim().is_empty() {
            "empty capture output".to_string()
        } else {
            out.stderr.trim().to_string()
        };
        return serde_json::json!({ "error": msg });
    }
    if !out.stderr.trim().is_empty() {
        return serde_json::json!({ "error": out.stderr.trim() });
    }
    serde_json::json!({ "image_base64": b64, "mime": "image/png" })
}

fn capture_hyperv_vm(a: HyperVArgs) -> Result<serde_json::Value, SdkError> {
    if a.vm_name.trim().is_empty() {
        return Err(SdkError::invalid_args("vm_name is required"));
    }
    let (w, h) = (a.width.unwrap_or(320).max(1), a.height.unwrap_or(240).max(1));
    host::log(&format!("[screenshot] capture_hyperv_vm {}", a.vm_name));
    let vm = ps_quote(a.vm_name.trim());
    let cmd = format!(
        r#"$ErrorActionPreference='Stop';Add-Type -AssemblyName System.Drawing;$ns='root\virtualization\v2';$vm=Get-CimInstance -Namespace $ns -ClassName Msvm_ComputerSystem -Filter "ElementName='{vm}' AND Caption='Virtual Machine'";if(-not $vm){{throw 'vm not found'}};$settings=Get-CimAssociatedInstance -InputObject $vm -Association Msvm_SettingsDefineState -ResultClassName Msvm_VirtualSystemSettingData;$svc=Get-CimInstance -Namespace $ns -ClassName Msvm_VirtualSystemManagementService;$r=Invoke-CimMethod -InputObject $svc -MethodName GetVirtualSystemThumbnailImage -Arguments @{{WidthPixels=[uint16]{w};HeightPixels=[uint16]{h};TargetSystem=$settings}};if($r.ReturnValue -ne 0){{throw "thumbnail failed $($r.ReturnValue)"}};$img=$r.ImageData;if(-not $img){{throw 'no image data'}};$bmp=New-Object System.Drawing.Bitmap({w},{h},[System.Drawing.Imaging.PixelFormat]::Format16bppRgb565);$rect=New-Object System.Drawing.Rectangle(0,0,{w},{h});$bd=$bmp.LockBits($rect,[System.Drawing.Imaging.ImageLockMode]::WriteOnly,[System.Drawing.Imaging.PixelFormat]::Format16bppRgb565);[System.Runtime.InteropServices.Marshal]::Copy($img,0,$bd.Scan0,$img.Length);$bmp.UnlockBits($bd);$ms=New-Object System.IO.MemoryStream;$bmp.Save($ms,[System.Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($ms.ToArray())"#
    );
    Ok(capture(&cmd))
}

fn capture_window(a: WindowArgs) -> Result<serde_json::Value, SdkError> {
    if a.title.trim().is_empty() {
        return Err(SdkError::invalid_args("title is required"));
    }
    host::log(&format!("[screenshot] capture_window {}", a.title));
    let title = ps_quote(a.title.trim());
    let cmd = format!(
        r#"$ErrorActionPreference='Stop';Add-Type -AssemblyName System.Drawing;{DPI_PRELUDE}Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public class Win {{
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr d,uint f);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 public struct RECT {{ public int L; public int T; public int R; public int B; }}
}}
'@
$p=Get-Process|Where-Object {{$_.MainWindowTitle -like '*{title}*' -and $_.MainWindowHandle -ne 0}}|Select-Object -First 1;if(-not $p){{throw 'window not found'}};$h=$p.MainWindowHandle;$r=New-Object Win+RECT;[Win]::GetWindowRect($h,[ref]$r)|Out-Null;$w=$r.R-$r.L;$ht=$r.B-$r.T;if($w -le 0 -or $ht -le 0){{throw 'bad window rect'}};$bmp=New-Object System.Drawing.Bitmap($w,$ht);$g=[System.Drawing.Graphics]::FromImage($bmp);$hdc=$g.GetHdc();[Win]::PrintWindow($h,$hdc,2)|Out-Null;$g.ReleaseHdc($hdc);$ms=New-Object System.IO.MemoryStream;$bmp.Save($ms,[System.Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($ms.ToArray())"#
    );
    Ok(capture(&cmd))
}

fn capture_desktop(a: DesktopArgs) -> Result<serde_json::Value, SdkError> {
    host::log("[screenshot] capture_desktop");
    let bounds = match a.monitor {
        Some(i) => format!("$s=[System.Windows.Forms.Screen]::AllScreens[{i}].Bounds;"),
        None => "$s=[System.Windows.Forms.SystemInformation]::VirtualScreen;".to_string(),
    };
    let cmd = format!(
        r#"$ErrorActionPreference='Stop';Add-Type -AssemblyName System.Drawing;Add-Type -AssemblyName System.Windows.Forms;{DPI_PRELUDE}{bounds}$bmp=New-Object System.Drawing.Bitmap($s.Width,$s.Height);$g=[System.Drawing.Graphics]::FromImage($bmp);$g.CopyFromScreen($s.X,$s.Y,0,0,$bmp.Size);$ms=New-Object System.IO.MemoryStream;$bmp.Save($ms,[System.Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($ms.ToArray())"#
    );
    Ok(capture(&cmd))
}

mtech_plugin! {
    id: "com.mastertech.screenshot",
    name: "Screenshot Capture",
    version: "0.2.0",
    heap: 32 * 1024 * 1024,
    tools: {
        /// Capture a Hyper-V VM console as a PNG via WMI GetVirtualSystemThumbnailImage (no guest Integration Components needed). Args: vm_name, width (default 320), height (default 240).
        capture_hyperv_vm(HyperVArgs) => capture_hyperv_vm,
        /// Capture the first top-level window whose title contains the substring, via PrintWindow (works on unfocused/background windows). DPI-aware.
        capture_window(WindowArgs) => capture_window,
        /// Capture the whole virtual desktop, or one monitor by 0-based index, via CopyFromScreen. DPI-aware.
        capture_desktop(DesktopArgs) => capture_desktop,
    }
}
