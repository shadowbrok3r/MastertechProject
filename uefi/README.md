# Mastertech UEFI app

A pre-OS diagnostic and firmware tool. It boots as `\EFI\BOOT\BOOTX64.EFI`, draws a
ratatui TUI over `EFI_SIMPLE_TEXT_OUTPUT`, and can be watched and driven remotely
from the admin console before any operating system exists.

Target `x86_64-unknown-uefi`, nightly (`rust-toolchain.toml`). This crate is its
own workspace — the repo root `exclude`s it — so build from inside `uefi/`.

```bash
cd uefi
cargo build --target x86_64-unknown-uefi            # -> target/x86_64-unknown-uefi/debug/uefi-app.efi
```

Copy that `.efi` to `\EFI\BOOT\BOOTX64.EFI` on a FAT volume to boot it.

---

## 1. Tabs

`Tab` / `→` / `l` next, `←` / `h` previous, digits `1`–`9` jump to the first nine,
`0` jumps to Log. **Flash is only reachable by `Tab`/`l`** — it sits past the digit
range on purpose so no existing digit shortcut moved.

| # | Tab | What it is |
|---|-----|-----------|
| 0 | Overview | One-screen identity + health summary |
| 1 | System | SMBIOS: manufacturer, product, serials, chassis, CPU |
| 2 | Memory | DIMMs, slots, ECC, totals |
| 3 | Firmware | ESRT, capsule flash, power gates, hardware error logs |
| 4 | BIOS | HII BIOS-setup audit (read-only) |
| 5 | Network | NIC drivers, DHCP lease, relay target, connect steps |
| 6 | Storage | NVMe Identify / SMART, SATA SMART via ATA pass-through |
| 7 | Stress | All-core stress via MP-Services |
| 8 | Order | Serial → PrestaShop order lookup |
| 9 | Readiness | Aggregate pass/fail |
| 10 | Diag | Boot diagnostics, RTC, BERT |
| 11 | Boot | `Boot####` entries, one-shot `BootNext` |
| 12 | Plugins | WASM plugins run in-firmware (wasmi) |
| 13 | Log | In-memory log ring — **first place to look when anything misbehaves** |
| 14 | Flash | BIOSLove model index: identify this machine, verify payloads, run vendor flashers |

## 2. Reading the status bar

```
NET 192.168.22.157   RLY axum.master-te~/ops   PRS arm   AGT auto   STR dir
```

| Field | Meaning |
|-------|---------|
| `NET` | IPv4 lease, or `down` |
| `RLY` | Relay target, truncated, then `/` and where it came from |
| `PRS` | Presence: `off` / `arm` (armed, not yet registered) / `reg` (registered) |
| `AGT` | Fleet agent auto-poll: `off` / `auto` |
| `STR` | Frame streaming: `off` / `rly` (via relay) / `dir` (direct socket) |

**`RLY` provenance suffixes**, weakest to strongest. A stronger source is never
overwritten by a weaker one:

| Suffix | Source |
|--------|--------|
| `/def` | Compile-time default, baked from `UEFI_TARGET_URL` |
| `/der` | Derived from a beacon's IPv4 (`http://<beacon-ip>:8082`) |
| `/adv` | Advertised by a v2 beacon, and on the beacon's own subnet |
| `/ops` | Operator: typed with `e`, or accepted with `y` |

`PRS arm` that never becomes `reg` means presence POSTs are failing. Check Log.

## 3. Networking — how the app reaches Mastertech

**Firmware has no DNS and no TLS.** It can only speak plain HTTP/1.1 to a LAN
IPv4. That single constraint explains the whole design.

```
UEFI app --HTTP--> preboot-relay:8082 --HTTPS--> axum.master-tech.app --> DB --> admin console
         \--TCP 9209--------------------------> admin console (direct, relay-free)
```

Two independent paths:

- **HTTP relay path.** Feeds the DB, so it is what makes a box appear in the admin
  console roster and what carries fingerprints, presence, and fleet commands.
  Requires `preboot-relay` running somewhere on the LAN.
- **Direct path (TCP 9209).** Frame streaming and remote input, no relay involved.
  Discovered from a UDP beacon. This is what `preboot_*` MCP tools drive.

The direct path can be up while the HTTP path is broken — the box is remotely
viewable but absent from the roster. That combination is common and confusing.

### Setting the relay

`preboot-relay` must be running. It is a separate crate:

```bash
cd preboot-relay && cargo run --release        # binds 0.0.0.0:8082 -> https://axum.master-tech.app
```

Verify from any machine on the LAN:

```bash
curl http://<relay-host>:8082/api/v1/qc/preboot/TEST/viewer     # expect {"viewer":false}
```

Then in the app, Network tab, `e`, type `http://<relay-host>:8082`, Enter.

**It must be `http://` and a bare IPv4.** An `https://` URL or a hostname cannot
work — the Network tab will show `sends by EFI HTTP (DNS+TLS)` and every request
fails `NOT_FOUND / token.status=NOT_READY`. Left alone the app derives the right
URL from a beacon (`/der`); it only ends up with an unusable one if an operator
typed it or pressed `y` on an off-subnet offer.

### Network tab keys

| Key | Action |
|-----|--------|
| `Shift+C` | Run all five connect steps |
| `c` | Bind NIC drivers |
| `d` | DHCP lease + arm presence |
| `n` | Toggle presence |
| `e` | Edit relay URL |
| `a` | Poll fleet commands once |
| `A` | Toggle auto-poll |
| `p` | Upload fingerprint now |
| `y` | Accept an offered off-subnet relay — **see the warning above** |
| `v` | Toggle relay frame streaming |
| `u` | Send the log ring to the relay |

### Machine identity

Everything keys on one serial, chosen by `effective_serial`, strongest first:

1. SMBIOS chassis serial
2. **ACPI MSDM OA3 key** — present on any Windows-licensed machine, and the id
   `connected_client` and PrestaShop both use
3. SMBIOS system serial, board serial, chassis asset tag

Values that are OEM filler (`Standard`, `Default`, `To be filled by O.E.M.`, `1558`,
…) are rejected by `is_placeholder_serial` so the OA3 key wins instead. This matters:
a junk serial is often shared by hundreds of `computer` rows, and the server links
a session to the first match — so a bad serial links the box to a stranger's record.
If you add a new OEM filler string, add it to that list.

## 4. Flash tab (BIOSLove)

Replaces the BIOSLove USB's `dir *.nsh` model picker. It identifies the machine,
verifies every byte against an index, then launches the vendor's own flasher.

### It needs `\bioslove\index.json`

Without it the tab shows `\bioslove\index.json not found on any volume`. The index
is generated off-box from the authoritative share:

```bash
cargo run -p bioslove-index -- --out index.json
```

Then place it, plus the payload tree, on the boot volume:

```
\EFI\BOOT\BOOTX64.EFI          the app
\bioslove\index.json           the index
\multiboot\BiosLove\laptop\…   payloads, as the share lays them out
\multiboot\BiosLove\Desktop\…
```

`payload_root` in the index (default `\multiboot\BiosLove`) is what the firmware
prepends, so a BIOSLove stick already has the tree in the right place — only
`index.json` is new. Override with `--payload-root` if you lay it out differently.

### Or let it fetch payloads over the LAN

Payloads do not have to be on the stick. `preboot-relay` serves them from the
share, content-addressed by digest, and the firmware stages what a step needs
into `\bioslove\cache\<folder>\`:

```bash
cd preboot-relay
BIOSLOVE_INDEX=../bioslove-index/index.json cargo run --release
#  -> preboot-relay: 585 payload digest(s) from … over \\opk-riv\…\BiosLove
```

Resolution order is **stick first, network second** — the relay is the least
reliable link in the chain, so a network failure degrades to "use what's here"
instead of blocking a flash. When anything has to be fetched, the *whole* step is
staged into the cache directory, because a vendor tool resolves its ROM relative
to its own device path and cannot straddle two directories.

Every fetched byte is digest-checked before use, and the route serves only
digests present in the index, so a request cannot name a file outside the share.
A 16 MiB ROM transfers in about 0.3 s on a gigabit LAN.

That means a working stick can be just the app plus the index — about 9 MB
instead of 5.5 GB — with the share as the single source of truth and no per-stick
drift. `BIOSLOVE_SHARE` overrides the share root.

To build a test image instead of a physical stick:

```bash
cargo run -p esp-image -- build --from ./staging --out esp.img --size-mb 256
cargo run -p esp-image -- list --image esp.img --dir /bioslove
```

### Flash tab keys

| Key | Action |
|-----|--------|
| `f` | Load / reload the index and re-detect |
| `e` | Search box — matches folder, aliases and model string across **both** sides |
| `Up`/`Down` | Select a model |
| `[` `]` | Move the step cursor within the selected recipe |
| `ENTER` | Verify the selected step — reads bytes, checks digests, validates the PE. **Runs nothing** |
| `p` | Confirm by hand that AC is connected, when the power state cannot be read |
| `F` `F` | Arm, then run the step |

Clearing the search box returns to the auto-detected list.

`ENTER` never loads or starts an image and never writes firmware. Its one side
effect is on the **USB volume**: a payload absent from the stick is fetched from
the relay and written to `\bioslove\cache\<folder>\`. Nothing is written to the
machine until `F` `F`.

### Matching

Chassis type picks laptop vs desktop. Then SMBIOS baseboard product → system
product → family → version, normalized to uppercase alphanumerics (`MS-16H5` finds
folder `MS16H5`). Three tiers, shown in the UI with the evidence:

- **exact** — a chassis token matched outright
- **family** — a wildcard folder (`PDxxSNx`) matched; these live in a separate
  `patterns` field so they can never match on folder name alone
- **partial** — substring; needs a human to confirm. OEMs prefix the family with a
  platform code (SMBIOS `PF5LUXG`, folder `LUXG`), so this tier carries real hits

Where two folders claim one token, the folder *named* for it outranks one that only
lists it as an alias. `bioslove-index` reports remaining ambiguity at generation
time under `match safety`.

### Delivery lanes

| Lane | Meaning |
|------|---------|
| `uefi` | Vendor `.efi` flasher — the app can run it |
| `capsule` | Spec-conformant FMP capsule — goes through `capsule.rs` |
| `dos_only` | Real-mode DOS flasher; needs the legacy BIOSLove boot |
| `in_bios_only` | Vendor's own in-setup updater (EZ Flash, M-Flash, Instant Flash) |
| `windows_only` | Needs a booted Windows |

Only the first two are launchable. For the rest the app identifies the folder and
prints the exact procedure rather than pretending it can help.

### Gates before anything runs

Every one must pass, and each prints its verdict:

- lane is launchable
- the index resolved this step's payloads
- AC/battery via `capsule::power_verdict` (50% floor on portables)
- board confirmed by SMBIOS, or an explicit operator override
- tool digest matches the index
- tool is a structurally valid EFI application (`pecheck`)
- every payload digest matches the index

Dimmed rows are entries whose payloads did not resolve — the index knows the
script references a file the share no longer has.

#### When the power state is unreadable

Live charge comes from the SBS Smart Battery over SMBus, and that lookup walks
PCI `00:1f.x` for vendor `0x8086` — **Intel only**. On an AMD laptop it finds
nothing, so the reading falls back to SMBIOS type 22, and boards that omit that
record (LUXG/`PF5LUXG` among them) leave the app with no power reading at all.

The gate distinguishes *measured and unfit* from *not measurable*:

- discharging, or below the 50% floor → refused outright, no override
- no reading at all → refused, but `p` records an operator attestation that AC is
  connected and downgrades it to a warning. The Flash tab shows `AC confirmed by
  operator` and the pre-flight report keeps a `WARN` line saying a human asserted it

`p` clears any prepared step, so press `ENTER` again after it.

### Multi-reboot recipes

36 steps power the machine off and 8 reboot. Position is saved to an NV UEFI
variable **before** the tool starts, so it survives. On the way back the Flash tab
reopens at the saved step with a `Resumed` banner, but only when the SMBIOS serial
matches and the recipe digest is unchanged — an index rebuild that altered the
recipe refuses the resume instead of continuing into a different sequence.

Every attempt also appends to `\bioslove\log\<serial>.jsonl` on the payload volume,
so a machine that dies mid-recipe still leaves a record.

## 5. Remote operation

With the direct link up, from the admin console or MCP.

### Reading state — prefer these

```
preboot_list_clients                              # serial, peer, idle seconds
preboot_get_status          {serial}              # the status bar, as data
preboot_get_logs            {serial, contains, limit}
preboot_get_system_info     {serial, section}
preboot_get_flash_state     {serial}
```

These return **JSON from firmware memory**, not a picture of a panel. That matters:
a TUI row is clipped to the panel it sits in and long reports scroll off with no
scrollback, so a screen read can silently omit the thing you are looking for. Each
one is a single round trip — no `stream_ctl`, no keypresses, no tab navigation.

- `preboot_get_logs` filters *before* taking the tail, so a ring flooded by one
  subsystem still yields the lines you want: `contains: "flash:"` for pre-flight
  verdicts, `"tcp:"` for networking, `"bioslove:"` for a running flash.
- `preboot_get_system_info` returns the same document the fingerprint push sends
  to axum. Pass `section` (`storage`, `diagnostics`, `bios_settings`, `identity`,
  `firmware_update`, …) — the whole document is large where the HII database is.
- `preboot_get_flash_state` carries the entire pre-flight report including the
  refusal reason, the power gate's verdict, and every step's payload digests.

Firmware answers on the main loop, so a box wedged in a blocking network call
answers late; the tools time out with the topic named rather than hanging.

### Driving the UI

```
preboot_stream_ctl  {serial, stream:true} # required before reading the screen
preboot_screen      {serial}              # the TUI as text rows
preboot_send_key    {serial, key}         # one keypress
preboot_type        {serial, text}        # literal text
```

Use these to *act*, or to see what the operator is seeing. `preboot_type` sends
**literal characters only** — `\b` arrives as a backslash and a `b`. Use
`preboot_send_key` with `backspace` or `delete` for editing.

### Wire

Query and answer are frame tags `0x0C`/`0x0D` carrying bincode `PbQuery` /
`PbQueryResult` ([tcp_protocol/src/lib.rs](../tcp_protocol/src/lib.rs)). The answer
body is a JSON *document*, so adding a topic changes only the firmware's
`answer_query` — no wire change, no fingerprint change. Both structs are pinned by
`shape_fingerprint` tests; a field added on one side without the other fails the
build rather than silently mis-decoding.

Firmware older than these tags ignores the query frame (unknown tags are dropped),
so the tool reports a timeout against a stale box rather than misreporting.

## 6. Troubleshooting

| Symptom | Cause |
|---------|-------|
| Freezes on `agent: registering + polling` | Relay unreachable. Each HTTP call blocks up to `TCP_CONNECT_TIMEOUT_MS` (10 s) and the poll re-fires every 5 s, starving the event loop |
| `relay: unreachable - muting relay HTTP for 40s` | The app noticed and backed off. Fix the relay URL |
| `request ERR … NOT_FOUND … (TLS/DNS/cert?)` | Target is `https://` or a hostname. Firmware has neither DNS nor TLS |
| `PRS arm` never reaches `reg` | Presence POSTs failing, or the serial is empty |
| Visible to `preboot_screen` but absent from the roster | Direct path up, HTTP relay path down. The roster comes from the DB |
| In the roster but nothing persists | The server's DB connection can wedge. `/register` and `/viewer` are in-memory and keep returning 200 while every write hangs — check `GET /api/v1/admin/info` for `db_connected` |
| `\bioslove\index.json not found` | Index was never generated or never copied. See §4 |
| Flash tab shows no match | Chassis token may be shorter than the partial threshold, or the folder has no `ver.txt` and so no aliases |
| `preboot_get_*` times out but `preboot_screen` works | The box is running firmware older than frame tag `0x0C`. Reflash the stick |
| A `preboot_get_*` tool isn't listed at all | The console binary predates it — rebuild and restart MasterTech, not just the firmware |

## 7. Layout

| File | Purpose |
|------|---------|
| `src/main.rs` | TUI, tabs, key handling, SMBIOS, network, fleet agent |
| `src/bioslove.rs` | Index schema, loader, SMBIOS matcher |
| `src/launch.rs` | `LoadImage`/`StartImage`, synthesized shell argv, device paths |
| `src/flashstate.rs` | NV-variable recipe position, on-volume JSONL log |
| `src/capsule.rs` | ESRT, `UpdateCapsule`, power gates, SHA-256 |
| `src/pecheck.rs` | Structural PE/COFF validation |
| `src/bootdiag.rs`, `src/order.rs`, `src/volsig.rs`, `src/smart.rs`, `src/hii.rs`, `src/stress.rs`, `src/netraw.rs`, `src/smolnet.rs`, `src/stream.rs`, `src/wasmrt.rs` | Per-tab subsystems |
| `src/bin/launchtest.rs` | QEMU harness for `launch.rs` and `flashstate.rs` |
| `qemu-drive.ps1` | Boot a staged image under OVMF and drive its TUI over TCP serial |

Related crates: `bioslove-index` (generates the index), `esp-image` (builds a real
FAT test image), `preboot-relay` (plain-HTTP → HTTPS relay).

## 8. Testing under QEMU

`cargo test` cannot run here — the test binary is a UEFI `.efi` and will not
execute on a host (`os error 129`). Logic that needs unit tests lives in the host
crates. For anything firmware-side, boot it:

```bash
cargo build --target x86_64-unknown-uefi
cargo run -p esp-image -- build --from ./staging --out esp.img --size-mb 256
./qemu-drive.ps1 -Dir . -Keys @('l*14','f') -Settle 16
```

QEMU/OVMF notes, all learned the hard way:

- `-bios` cannot load the 3.5 MB split OVMF `CODE` image — use `if=pflash`.
- `file=` treats a drive-letter colon as a protocol; run from the image's directory
  with relative paths.
- `fat:ro:` fails with "Block node is read-only"; `fat:rw:` works but **aborts QEMU
  on directory creation** and corrupts host-side files. Use `esp-image` for
  anything that writes.
- OVMF boots its internal shell rather than `\EFI\BOOT\BOOTX64.EFI` — drop a
  `startup.nsh` in the volume root.
- A stdin file redirect never reaches ConIn; drive it over `-serial tcp:`.
