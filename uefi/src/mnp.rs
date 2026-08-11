//! EFI Managed Network Protocol client — the demuxed way onto the NIC.
//!
//! Raw SimpleNetwork receive is a single queue: every frame goes to exactly one
//! consumer, so this app and the firmware's own stack (ArpDxe, Dhcp4Dxe, ...)
//! steal from each other — unicast ARP/DHCP replies vanish for whoever loses
//! the race. MnpDxe multiplexes one SNP among many children and hands each a
//! copy of every matching frame, ending the theft in both directions. Frames
//! are still sent and parsed whole (Ethernet header included), so the callers'
//! frame builders are unchanged; only the transport differs.
//!
//! `uefi-raw` 0.15 has no MNP bindings, so the protocol surface is defined here
//! against the UEFI spec (§24.1, Managed Network Protocol).

use core::ffi::c_void;
use core::ptr;
use core::time::Duration;

use uefi::boot::{self};
use uefi::proto::unsafe_protocol;
use uefi_raw::protocol::driver::ServiceBindingProtocol;
use uefi_raw::table::boot::{EventType, Tpl};
use uefi_raw::{Boolean, Status};

use crate::logln;
use crate::protoguard::{self, Held};

#[repr(C)]
pub struct MnpConfigData {
    received_queue_timeout_value: u32,
    transmit_queue_timeout_value: u32,
    protocol_type_filter: u16,
    enable_unicast_receive: Boolean,
    enable_multicast_receive: Boolean,
    enable_broadcast_receive: Boolean,
    enable_promiscuous_receive: Boolean,
    flush_queues_on_reset: Boolean,
    enable_receive_timestamps: Boolean,
    disable_background_polling: Boolean,
}

#[repr(C)]
pub struct MnpCompletionToken {
    event: uefi_raw::Event,
    status: Status,
    /// RxData or TxData, depending on the call the token was passed to.
    packet: *mut c_void,
}

#[repr(C)]
struct MnpReceiveData {
    /// EFI_TIME by layout; never read.
    timestamp: [u8; 16],
    recycle_event: uefi_raw::Event,
    packet_length: u32,
    header_length: u32,
    address_length: u32,
    data_length: u32,
    broadcast_flag: Boolean,
    multicast_flag: Boolean,
    promiscuous_flag: Boolean,
    protocol_type: u16,
    destination_address: *mut c_void,
    source_address: *mut c_void,
    media_header: *mut c_void,
    packet_data: *mut c_void,
}

#[repr(C)]
struct MnpFragmentData {
    fragment_length: u32,
    fragment_buffer: *mut c_void,
}

#[repr(C)]
struct MnpTransmitData1 {
    destination_address: *mut c_void,
    source_address: *mut c_void,
    protocol_type: u16,
    data_length: u32,
    header_length: u16,
    fragment_count: u16,
    fragment_table: [MnpFragmentData; 1],
}

#[repr(C)]
pub struct ManagedNetworkProtocol {
    get_mode_data: unsafe extern "efiapi" fn(
        this: *mut Self,
        mnp_config: *mut MnpConfigData,
        snp_mode: *mut c_void,
    ) -> Status,
    configure: unsafe extern "efiapi" fn(this: *mut Self, config: *const MnpConfigData) -> Status,
    mcast_ip_to_mac: unsafe extern "efiapi" fn(
        this: *mut Self,
        ipv6: Boolean,
        ip: *const c_void,
        mac: *mut c_void,
    ) -> Status,
    groups: unsafe extern "efiapi" fn(this: *mut Self, join: Boolean, mac: *const c_void) -> Status,
    transmit: unsafe extern "efiapi" fn(this: *mut Self, token: *mut MnpCompletionToken) -> Status,
    receive: unsafe extern "efiapi" fn(this: *mut Self, token: *mut MnpCompletionToken) -> Status,
    cancel: unsafe extern "efiapi" fn(this: *mut Self, token: *mut MnpCompletionToken) -> Status,
    poll: unsafe extern "efiapi" fn(this: *mut Self) -> Status,
}

#[unsafe_protocol("f36ff770-a7e1-42cf-9ed2-56f0f271f44c")]
struct MnpSb(ServiceBindingProtocol);

#[unsafe_protocol("7ab33a91-ace5-4326-b572-e7ee33d39f16")]
struct Mnp(ManagedNetworkProtocol);

/// Ethernet media header length; frames are handed over fully built.
const MEDIA_HEADER: usize = 14;

/// Set when a firmware's MNP rejects the caller-built-header transmit form;
/// every later open then falls straight through to raw SNP instead of failing
/// each operation against a transport that cannot send.
static POISONED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Budget for one transmit token to complete.
const TX_WAIT_MS: u64 = 1_000;

/// Signal an event through raw boot services (recycle events arrive raw).
fn signal_raw(event: uefi_raw::Event) {
    if event.is_null() {
        return;
    }
    let Some(st) = uefi::table::system_table_raw() else {
        return;
    };
    // SAFETY: the system table and its boot services are live until exit.
    unsafe {
        let bt = (*st.as_ptr()).boot_services;
        let _ = ((*bt).signal_event)(event);
    }
}

/// One configured MNP child on the first managed NIC.
pub struct MnpNet {
    sb: Held<MnpSb>,
    child: uefi_raw::Handle,
    mnp: Held<Mnp>,
    tx_event: Option<uefi::Event>,
    rx_event: Option<uefi::Event>,
    /// Heap-pinned: MnpDxe holds this pointer while a receive is outstanding.
    rx_token: Box<MnpCompletionToken>,
    rx_pending: bool,
    pub mac: [u8; 6],
    pub media_present: bool,
}

impl MnpNet {
    /// The protocol interface lives in firmware memory, stable across moves.
    fn this(&self) -> *mut ManagedNetworkProtocol {
        let p: *const ManagedNetworkProtocol = &self.mnp.0;
        p.cast_mut()
    }

    pub fn open() -> Result<Self, String> {
        if POISONED.load(core::sync::atomic::Ordering::Relaxed) {
            return Err("MNP transmit rejected earlier".into());
        }
        let sbh = *boot::find_handles::<MnpSb>()
            .map_err(|e| format!("find MNP-SB: {e:?}"))?
            .first()
            .ok_or("no MNP service binding")?;

        // MnpDxe installs its service binding on the NIC handle, so the same
        // handle carries SimpleNetwork; read MAC + media from it (read-only).
        let (mac, media_present) = {
            let snp = protoguard::get::<uefi::proto::network::snp::SimpleNetwork>(sbh)
                .map_err(|e| format!("read SNP mode: {e:?}"))?;
            let m = snp.mode();
            let a = m.current_address.0;
            ([a[0], a[1], a[2], a[3], a[4], a[5]], bool::from(m.media_present))
        };

        let mut sb =
            protoguard::get::<MnpSb>(sbh).map_err(|e| format!("open MNP-SB: {e:?}"))?;
        let mut child: uefi_raw::Handle = ptr::null_mut();
        let st = unsafe { (sb.0.create_child)(&mut sb.0, &mut child) };
        if st != Status::SUCCESS {
            return Err(format!("MNP create_child: {st:?}"));
        }
        let child_handle = match unsafe { uefi::Handle::from_ptr(child) } {
            Some(h) => h,
            None => {
                let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
                return Err("null MNP child".into());
            }
        };
        let mnp = match protoguard::get::<Mnp>(child_handle) {
            Ok(p) => p,
            Err(e) => {
                let _ = unsafe { (sb.0.destroy_child)(&mut sb.0, child) };
                return Err(format!("open MNP child: {e:?}"));
            }
        };

        let mut net = Self {
            sb,
            child,
            mnp,
            tx_event: None,
            rx_event: None,
            rx_token: Box::new(MnpCompletionToken {
                event: ptr::null_mut(),
                status: Status::NOT_READY,
                packet: ptr::null_mut(),
            }),
            rx_pending: false,
            mac,
            media_present,
        };

        // Tokens require an event even when completion is read by polling the
        // status field; one per direction, reused across calls.
        for slot in [&mut net.tx_event, &mut net.rx_event] {
            match unsafe { boot::create_event(EventType::empty(), Tpl::CALLBACK, None, None) } {
                Ok(e) => *slot = Some(e),
                Err(e) => return Err(format!("MNP event: {e:?}")),
            }
        }

        let config = MnpConfigData {
            received_queue_timeout_value: 0,
            transmit_queue_timeout_value: 0,
            // 0 accepts every ethertype: DHCP/UDP need 0x0800, ARP 0x0806.
            protocol_type_filter: 0,
            enable_unicast_receive: Boolean::from(true),
            enable_multicast_receive: Boolean::from(false),
            enable_broadcast_receive: Boolean::from(true),
            enable_promiscuous_receive: Boolean::from(false),
            flush_queues_on_reset: Boolean::from(false),
            enable_receive_timestamps: Boolean::from(false),
            disable_background_polling: Boolean::from(false),
        };
        let st = unsafe { ((*net.this()).configure)(net.this(), &config) };
        if st != Status::SUCCESS {
            return Err(format!("MNP configure: {st:?}"));
        }
        Ok(net)
    }

    /// Send one fully built Ethernet frame and wait for the token to complete.
    /// Caller-built header form: DestinationAddress null, HeaderLength = 14,
    /// DataLength excludes the header, fragment carries the whole frame.
    pub fn transmit(&self, frame: &[u8]) -> Result<(), String> {
        if frame.len() <= MEDIA_HEADER {
            return Err("frame shorter than the media header".into());
        }
        // Own the bytes so an unrecoverable token can be leaked without ever
        // leaving MnpDxe a pointer into a caller's freed buffer.
        let owned: Box<[u8]> = frame.into();
        let mut txd = Box::new(MnpTransmitData1 {
            destination_address: ptr::null_mut(),
            source_address: ptr::null_mut(),
            protocol_type: 0,
            data_length: (owned.len() - MEDIA_HEADER) as u32,
            header_length: MEDIA_HEADER as u16,
            fragment_count: 1,
            fragment_table: [MnpFragmentData {
                fragment_length: owned.len() as u32,
                fragment_buffer: owned.as_ptr() as *mut c_void,
            }],
        });
        let mut token = Box::new(MnpCompletionToken {
            event: self.tx_event.as_ref().map_or(ptr::null_mut(), |e| e.as_ptr()),
            status: Status::NOT_READY,
            packet: (&mut *txd as *mut MnpTransmitData1).cast(),
        });

        let st = unsafe { ((*self.this()).transmit)(self.this(), &mut *token) };
        if st != Status::SUCCESS {
            if st == Status::INVALID_PARAMETER {
                POISONED.store(true, core::sync::atomic::Ordering::Relaxed);
                logln("mnp: transmit form rejected - demoting to raw SNP".into());
            }
            return Err(format!("MNP transmit: {st:?}"));
        }
        let mut waited = 0u64;
        loop {
            let _ = unsafe { ((*self.this()).poll)(self.this()) };
            let now = unsafe { ptr::read_volatile(&token.status) };
            if now != Status::NOT_READY {
                return if now == Status::SUCCESS {
                    Ok(())
                } else {
                    Err(format!("MNP tx completed {now:?}"))
                };
            }
            if waited >= TX_WAIT_MS {
                let _ = unsafe { ((*self.this()).cancel)(self.this(), &mut *token) };
                for _ in 0..200 {
                    let _ = unsafe { ((*self.this()).poll)(self.this()) };
                    if unsafe { ptr::read_volatile(&token.status) } != Status::NOT_READY {
                        return Err("MNP tx timeout (cancelled)".into());
                    }
                    boot::stall(Duration::from_millis(1));
                }
                // Still owned by MnpDxe: leak rather than free under it.
                core::mem::forget((token, txd, owned));
                return Err("MNP tx stuck (token leaked)".into());
            }
            boot::stall(Duration::from_millis(1));
            waited += 1;
        }
    }

    /// Poll for one frame, up to `timeout_ms` (0 = single non-blocking pass).
    /// Returns the frame length and the milliseconds actually spent waiting.
    /// The receive token stays outstanding across calls, so frames arriving
    /// between calls queue inside MnpDxe instead of being lost.
    pub fn recv(&mut self, buf: &mut [u8], timeout_ms: u64) -> (Option<usize>, u64) {
        let mut waited = 0u64;
        loop {
            if !self.rx_pending {
                self.rx_token.event =
                    self.rx_event.as_ref().map_or(ptr::null_mut(), |e| e.as_ptr());
                self.rx_token.status = Status::NOT_READY;
                self.rx_token.packet = ptr::null_mut();
                let st = unsafe { ((*self.this()).receive)(self.this(), &mut *self.rx_token) };
                if st != Status::SUCCESS {
                    return (None, waited);
                }
                self.rx_pending = true;
            }
            let _ = unsafe { ((*self.this()).poll)(self.this()) };
            let now = unsafe { ptr::read_volatile(&self.rx_token.status) };
            if now != Status::NOT_READY {
                self.rx_pending = false;
                if now == Status::SUCCESS && !self.rx_token.packet.is_null() {
                    let n = unsafe { self.take_frame(buf) };
                    if let Some(n) = n {
                        return (Some(n), waited);
                    }
                    // Oversized frame dropped; token resubmits next pass.
                }
            }
            if waited >= timeout_ms {
                return (None, waited);
            }
            boot::stall(Duration::from_millis(5));
            waited += 5;
        }
    }

    /// Copy the completed RxData out as one contiguous frame and recycle the
    /// MNP-owned buffer. None when `buf` cannot hold it.
    unsafe fn take_frame(&mut self, buf: &mut [u8]) -> Option<usize> {
        let rx = unsafe { &*(self.rx_token.packet as *const MnpReceiveData) };
        let hdr = rx.header_length as usize;
        let data = rx.data_length as usize;
        let total = hdr + data;
        let ok = total <= buf.len() && !rx.media_header.is_null() && !rx.packet_data.is_null();
        if ok {
            unsafe {
                ptr::copy_nonoverlapping(rx.media_header as *const u8, buf.as_mut_ptr(), hdr);
                ptr::copy_nonoverlapping(
                    rx.packet_data as *const u8,
                    buf.as_mut_ptr().add(hdr),
                    data,
                );
            }
        }
        signal_raw(rx.recycle_event);
        self.rx_token.packet = ptr::null_mut();
        ok.then_some(total)
    }
}

impl Drop for MnpNet {
    fn drop(&mut self) {
        // Null token cancels every outstanding token; the reset then returns
        // the instance to unconfigured before the child handle goes away.
        unsafe {
            let _ = ((*self.this()).cancel)(self.this(), ptr::null_mut());
            let _ = ((*self.this()).configure)(self.this(), ptr::null());
        }
        for ev in [self.tx_event.take(), self.rx_event.take()].into_iter().flatten() {
            let _ = boot::close_event(ev);
        }
        let _ = unsafe { (self.sb.0.destroy_child)(&mut self.sb.0, self.child) };
        // self.mnp (Held) drops after this body; its close on the destroyed
        // child is swallowed rather than asserted.
    }
}

/// Log-once guard for the transport choice.
static ANNOUNCED: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Log the chosen transport when it changes (1 = MNP, 2 = raw SNP).
pub fn announce(kind: u8, detail: &str) {
    if ANNOUNCED.swap(kind, core::sync::atomic::Ordering::Relaxed) != kind {
        logln(format!("netraw: transport = {detail}"));
    }
}
