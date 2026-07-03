# pxe-bench on the Ubuntu bench host — full setup

Netboot a dead/incoming machine straight into Mastertech PE. `pxe-bench` runs on
the Ubuntu box (192.168.22.7), serves the payload over TFTP + HTTP, and coexists
with your existing DHCP (it never hands out addresses).

The `.img`-backed Samba share (`\\192.168.22.7\Mtech`) is the **staging channel**:
you drop files in from Windows, and `pxe-bench` — reading the same directory
locally on Ubuntu — serves them out. All facts below verified against ipxe.org,
github.com/ipxe, and learn.microsoft.com (2026-07).

---

## 0. One decision up front: where is the .img mounted?

Everything hangs off one path. Find the local mount of the share on Ubuntu:

```bash
findmnt | grep -i mtech        # or: grep -i 'path' /etc/samba/smb.conf
```

This guide assumes the share's local path is **`/srv/mtech`**. If it's different
(e.g. `/mnt/mtech`), substitute it everywhere below and in `pxe-bench.toml`.

Create the layout inside it (persists in the .img, editable from Windows):

```bash
sudo mkdir -p /srv/mtech/pxe/tftp /srv/mtech/pxe/http/media
```

---

## 1. Build pxe-bench on the Ubuntu box

The crate is self-contained (concrete deps, no workspace needed). Copy just the
`pxe-bench/` folder over — easiest via the share from Windows:

```
copy the repo's  pxe-bench\  folder  ->  \\192.168.22.7\Mtech\pxe\pxe-bench\
```

Then on Ubuntu:

```bash
# Rust toolchain (skip if already present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

cd /srv/mtech/pxe/pxe-bench
cargo build --release
cp target/release/pxe-bench /srv/mtech/pxe/pxe-bench
```

(If `/srv/mtech` is a noexec mount, build in `~` and copy the binary to
`/srv/mtech/pxe/`. The binary just needs to *run* from there; the payload dirs
are what must live in the share.)

---

## 2. Stage the iPXE payload

Copy `deploy/pxe-bench.toml` and `deploy/stage-payload.sh` into `/srv/mtech/pxe/`,
then:

```bash
cd /srv/mtech/pxe
chmod +x stage-payload.sh
./stage-payload.sh /srv/mtech/pxe
```

That downloads (verified URLs):

| File | From | Into | For |
|---|---|---|---|
| `undionly.kpxe` | `boot.ipxe.org/undionly.kpxe` | `tftp/` | Legacy BIOS PXE |
| `snponly.efi` | `boot.ipxe.org/x86_64-efi/snponly.efi` | `tftp/` | x64 UEFI (default) |
| `ipxe.efi` | `boot.ipxe.org/x86_64-efi/ipxe.efi` | `tftp/` | x64 UEFI (fallback) |
| `wimboot` | `github.com/ipxe/wimboot/releases/latest/download/wimboot` | `http/` | WinPE loader (v2.9.0+) |

> **Gotcha the fact-check caught:** the UEFI binaries are under `x86_64-efi/`, not
> the site root, and `wimboot` comes from GitHub — `boot.ipxe.org/wimboot` 404s.
> The staging script already uses the right URLs.

`boot.ipxe` is auto-generated into `http/` on first run of pxe-bench (minimal
wimboot recipe: `kernel wimboot` + `initrd …/media/sources/boot.wim boot.wim`).

---

## 3. Build the Mastertech WinPE image (on Windows, needs the ADK)

### 3a. Install the ADK + WinPE add-on (two separate installers)

From <https://learn.microsoft.com/en-us/windows-hardware/get-started/adk-install>:

1. **Windows ADK** (`adksetup.exe`) — install first.
2. **Windows PE add-on for the ADK** (`adkwinpesetup.exe`) — separate download,
   install second. Since Win10 1809 WinPE is *not* bundled with the ADK.

> Apply the current ADK **servicing patch** before building — CVE-2026-25166 is a
> WSIM RCE (CVSS 7.8) in unpatched installs. For the Dec-2024 ADK (10.1.26100.2454)
> that's KB5079391; match the KB to your ADK build.

The WinPE optional components (OCs) install to (confirm the exact path — it's the
`(x86)` kit dir on most hosts):

```
C:\Program Files (x86)\Windows Kits\10\Assessment and Deployment Kit\Windows Preinstallation Environment\amd64\WinPE_OCs\
```

### 3b. Build boot.wim (elevated "Deployment and Imaging Tools Environment")

```bat
copype amd64 C:\WinPE_MT

set OCS=C:\Program Files (x86)\Windows Kits\10\Assessment and Deployment Kit\Windows Preinstallation Environment\amd64\WinPE_OCs
Dism /Mount-Image /ImageFile:"C:\WinPE_MT\media\sources\boot.wim" /Index:1 /MountDir:"C:\WinPE_MT\mount"

rem WMI is required for MasterTech's hardware queries. Add each neutral cab then
rem its _en-us cab, in dependency order. (NetFX/Scripting/PowerShell/StorageWMI
rem are optional — add only if your workflows need them.)
Dism /Add-Package /Image:"C:\WinPE_MT\mount" /PackagePath:"%OCS%\WinPE-WMI.cab"
Dism /Add-Package /Image:"C:\WinPE_MT\mount" /PackagePath:"%OCS%\en-us\WinPE-WMI_en-us.cab"
```

Dependency order if you add more: **WMI → NetFX → Scripting → PowerShell → StorageWMI**.
OC cabs must come from the **same ADK build/arch** as boot.wim.

### 3c. Inject the client + auto-launch, then commit

`MasterTech.exe` must be present and *runnable* inside WinPE. Two things:

**(i) The runtime-DLL gotcha.** WinPE ships the UCRT but **not** the VC++ runtime,
so a default Rust MSVC build fails at launch with `VCRUNTIME140.dll not found`.
Fix it by building the client with the **static CRT** (preferred):

```powershell
# when building MasterTech.exe:
$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build -p MasterTech --profile release-fast
```

or, alternatively, copy the whole VC++ 2015-2022 redist set into the image (copy
**all three** or you get a version-mismatch crash):

```bat
copy "%VCToolsRedistDir%\x64\Microsoft.VC143.CRT\vcruntime140.dll"   C:\WinPE_MT\mount\Windows\System32\
copy "%VCToolsRedistDir%\x64\Microsoft.VC143.CRT\vcruntime140_1.dll" C:\WinPE_MT\mount\Windows\System32\
copy "%VCToolsRedistDir%\x64\Microsoft.VC143.CRT\msvcp140.dll"       C:\WinPE_MT\mount\Windows\System32\
```

**(ii) Inject + auto-start:**

```bat
copy MasterTech.exe C:\WinPE_MT\mount\Windows\System32\
> C:\WinPE_MT\mount\Windows\System32\startnet.cmd  echo wpeinit
>> C:\WinPE_MT\mount\Windows\System32\startnet.cmd echo MasterTech.exe --cpu

Dism /Unmount-Image /MountDir:"C:\WinPE_MT\mount" /Commit
```

`--cpu` forces the egui_skia software renderer (no GPU driver in PE); the client
falls back to terminal mode on its own if that fails, and SNTP-syncs the clock
before TLS (`database::clock_sync`) so the stale-PE-clock cert problem is handled.

### 3d. Copy the media tree to the share

From Windows, copy the whole `C:\WinPE_MT\media\` tree into the share so these
exist on the Ubuntu side (`copype` layout, verified):

```
\\192.168.22.7\Mtech\pxe\http\media\sources\boot.wim
\\192.168.22.7\Mtech\pxe\http\media\Boot\boot.sdi
\\192.168.22.7\Mtech\pxe\http\media\Boot\BCD              (BIOS BCD)
\\192.168.22.7\Mtech\pxe\http\media\EFI\Microsoft\Boot\BCD  (UEFI BCD)
```

The minimal wimboot recipe only needs `sources/boot.wim`, but copy the full tree
so the explicit-BCD fallback works too.

---

## 4. Install the service

Copy `deploy/pxe-bench.service` to the box and enable it:

```bash
sudo cp /srv/mtech/pxe/pxe-bench.service /etc/systemd/system/pxe-bench.service
sudo systemctl daemon-reload
sudo systemctl enable --now pxe-bench
journalctl -u pxe-bench -f
```

Open the firewall (ufw shown; skip if inactive):

```bash
sudo ufw allow 67/udp     # ProxyDHCP
sudo ufw allow 4011/udp   # PXE boot server
sudo ufw allow 69/udp     # TFTP
sudo ufw allow 7777/tcp   # HTTP boot payload
```

---

## 5. First boot test

Point a bench machine (or a Hyper-V VM on the bench segment — Gen1 for BIOS, Gen2
with **Secure Boot off** for UEFI) at network boot. Watch `journalctl -u pxe-bench`:

```
PXE DISCOVER from aa:bb:cc:dd:ee:ff (arch Some(7), ipxe false) -> offering snponly.efi
tftp: <ip> <- snponly.efi (…)
PXE DISCOVER from … (ipxe true) -> offering http://192.168.22.7:7777/boot.ipxe
http: 200 OK /boot.ipxe
http: 200 OK /wimboot
http: 206 Partial Content /media/sources/boot.wim
```

That sequence = firmware → TFTP iPXE → HTTP chain → wimboot → WinPE. The machine
then boots into Mastertech and links to the admin console.

---

## Must-know gotchas (from the verification pass)

- **Secure Boot** blocks the unsigned `snponly.efi`/`ipxe.efi` — the boot silently
  fails. **Disable Secure Boot** on the bench, or chain through a Microsoft-signed
  `shimx64.efi` (on `boot.ipxe.org` root) that trusts iPXE. There is no
  Microsoft-signed stock iPXE binary.
- **Port 67 conflict:** if the Ubuntu box *itself* also runs a DHCP server, two
  processes can't both bind UDP 67. pxe-bench is ProxyDHCP (boot info only), so the
  shop's real DHCP should be a *different* host (router/server). Check with
  `sudo ss -ulpn | grep ':67'` before starting.
- **TFTP ephemeral ports:** the initial NBP transfer is UDP 69, then data moves to
  a random high source port. A stateful firewall needs the TFTP conntrack helper
  (`sudo modprobe nf_conntrack_tftp`) or transfers hang after the first packet.
  Everything after iPXE loads uses HTTP, which avoids this.
- **VC++ runtime in WinPE** — see 3c(i). The #1 reason a freshly-built client
  "does nothing" in PE.
- **WinPE OC version match:** OC cabs (and their `_en-us` variants) must match the
  ADK build/arch of the boot.wim, installed in dependency order.

## Where to extend

Boot events are logged, not yet persisted. To record every bench boot to
SurrealDB/admin console, hook the `PXE DISCOVER … -> offering` line in
[dhcp.rs](../src/dhcp.rs) (MAC + arch are known there) — a natural companion to
the crash-intel/fleet features.
