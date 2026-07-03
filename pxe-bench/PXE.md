# pxe-bench — bench PXE appliance

Plug a dead/incoming machine into the bench VLAN, power it on, pick network
boot (or let a blank disk fall through to PXE), and it boots straight into
Mastertech PE with the client auto-linking to the admin console. No USB sticks.

## How it works

```
machine ──DHCP DISCOVER──▶ shop DHCP (address)  +  pxe-bench :67 (ProxyDHCP: boot info only)
        ──REQUEST :4011──▶ pxe-bench (ACK: undionly.kpxe / ipxe.efi by client arch)
        ──TFTP :69───────▶ iPXE binary (~90 KB)
 iPXE   ──re-DHCP────────▶ pxe-bench detects user-class "iPXE" → hands boot.ipxe URL
        ──HTTP :7777─────▶ wimboot + BCD + boot.sdi + boot.wim (fast, resumable)
 WinPE  ──runs MasterTech.exe → SNTP clock-sync → egui/terminal fallback → links to admin
```

ProxyDHCP never allocates addresses, so it coexists with the shop's existing
DHCP server — nothing on the main network changes. Machines only netboot if
they're on the bench segment AND ask for PXE.

## One-time payload setup

For a step-by-step Ubuntu deployment (build, service, ADK/WinPE), see
[deploy/UBUNTU-SETUP.md](deploy/UBUNTU-SETUP.md). `deploy/stage-payload.sh`
downloads the binaries below with the correct URLs; the summary:

1. **iPXE binaries** → `tftp/`
   - BIOS: `https://boot.ipxe.org/undionly.kpxe`
   - x64 UEFI (default): `https://boot.ipxe.org/x86_64-efi/snponly.efi`
   - x64 UEFI (fallback): `https://boot.ipxe.org/x86_64-efi/ipxe.efi`
   The UEFI binaries live under `x86_64-efi/`, not the site root.

2. **wimboot** → `http/`
   Download from `https://github.com/ipxe/wimboot/releases/latest/download/wimboot`
   (the plain binary). `boot.ipxe.org/wimboot` is not a valid path.

3. **Mastertech PE media** → `http/media/`
   Build WinPE with the ADK (Deployment and Imaging Tools):

   ```bat
   copype amd64 C:\WinPE_MT
   Dism /Mount-Image /ImageFile:C:\WinPE_MT\media\sources\boot.wim /Index:1 /MountDir:C:\WinPE_MT\mount
   Dism /Image:C:\WinPE_MT\mount /Add-Package /PackagePath:"...\WinPE-WMI.cab"
   Dism /Image:C:\WinPE_MT\mount /Add-Package /PackagePath:"...\WinPE-NetFX.cab"
   rem Inject the client + startnet auto-launch:
   copy MasterTech.exe C:\WinPE_MT\mount\Windows\System32\
   echo wpeinit >  C:\WinPE_MT\mount\Windows\System32\startnet.cmd
   echo MasterTech.exe --cpu >> C:\WinPE_MT\mount\Windows\System32\startnet.cmd
   Dism /Unmount-Image /MountDir:C:\WinPE_MT\mount /Commit
   ```

   Copy the whole `C:\WinPE_MT\media` tree to `http/media/` so these exist:
   - `http/media/Boot/BCD`
   - `http/media/Boot/boot.sdi`
   - `http/media/sources/boot.wim`

   Notes:
   - `--cpu` forces the egui_skia software renderer (PE has no GPU driver);
     the client falls back to terminal mode on its own if that fails too.
   - The client SNTP-syncs the clock before TLS (`database::clock_sync`), so
     the stale-PE-clock certificate problem is already handled.

4. **boot.ipxe** — auto-generated into `http_root` on first run (edit freely).

## Running

```powershell
cargo build -p pxe-bench --release
.\target\release\pxe-bench.exe          # writes pxe-bench.toml template on first run
# edit pxe-bench.toml → set server_ip to the bench NIC's IPv4
.\target\release\pxe-bench.exe
```

Windows firewall (run elevated, once):

```powershell
New-NetFirewallRule -DisplayName "pxe-bench DHCP"  -Direction Inbound -Protocol UDP -LocalPort 67,4011 -Action Allow
New-NetFirewallRule -DisplayName "pxe-bench TFTP"  -Direction Inbound -Protocol UDP -LocalPort 69   -Action Allow
New-NetFirewallRule -DisplayName "pxe-bench HTTP"  -Direction Inbound -Protocol TCP -LocalPort 7777 -Action Allow
```

TFTP transfers also negotiate an ephemeral source port per RFC 1350 — if
transfers stall after the first packet, allow outbound UDP from the appliance
or scope the rules to the bench subnet.

## Bench networking

- Give the bench segment its own VLAN/switch so only intake machines see the
  ProxyDHCP offers.
- The shop DHCP must reach that segment (or run any DHCP there) — pxe-bench
  itself does not lease addresses.
- `server_ip` must be the appliance's address **on the bench segment**.

## Verifying without hardware

- `cargo test -p pxe-bench` covers DHCP packet round-trips, arch → boot-file
  selection, the iPXE redirect, TFTP option parsing, and path-traversal guards.
- A VM (Hyper-V Gen1 for BIOS, Gen2 with Secure Boot off for UEFI) on the
  bench segment is the fastest end-to-end test: watch the pxe-bench log for
  `PXE DISCOVER … -> offering`, the TFTP fetch, then HTTP hits for
  wimboot/boot.wim.

## Known gaps (v1)

- No per-MAC boot menus — every PXE client on the segment gets Mastertech PE.
- Secure Boot UEFI clients need a signed shim chain; the stock `ipxe.efi` is
  unsigned. Disable Secure Boot on the bench or add a shim later.
- Boot events are logged, not yet written to SurrealDB/admin console. Hook
  candidates: `dhcp.rs` at the `PXE DISCOVER` log line (MAC + arch known).
