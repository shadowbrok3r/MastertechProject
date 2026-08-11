//! Protocol opens whose close is allowed to fail.
//!
//! Firmware removes an agent's GET_PROTOCOL open record when the interface is
//! uninstalled or the controller is disconnected, and destroys service-binding
//! children outright — both routine while the IPv4 stack reconfigures during
//! DHCP/PXE. `uefi::boot::ScopedProtocol` asserts CloseProtocol returned
//! SUCCESS, so its `Drop` panics once that has happened. [`Held`] issues the
//! same close and discards the status.

use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};
use core::ptr;

use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::proto::{Protocol, ProtocolPointer};
use uefi::{Guid, Handle};

/// An open protocol interface whose close cannot panic. The field is named so
/// that tuple-field access on the wrapped protocol still autoderefs through.
pub struct Held<P: Protocol + ?Sized> {
    proto: ManuallyDrop<ScopedProtocol<P>>,
}

impl<P: Protocol + ?Sized> Held<P> {
    /// Take over an already-open protocol.
    pub fn new(proto: ScopedProtocol<P>) -> Self {
        Self { proto: ManuallyDrop::new(proto) }
    }
}

impl<P: Protocol + ?Sized> Deref for Held<P> {
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.proto
    }
}

impl<P: Protocol + ?Sized> DerefMut for Held<P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.proto
    }
}

impl<P: Protocol + ?Sized> Drop for Held<P> {
    fn drop(&mut self) {
        // ManuallyDrop suppresses ScopedProtocol's asserting close.
        close(self.proto.open_params(), &P::GUID);
    }
}

/// Open `P` on `handle` non-exclusively, as the loaded image.
pub fn get<P: ProtocolPointer + ?Sized>(handle: Handle) -> uefi::Result<Held<P>> {
    let params = OpenProtocolParams { handle, agent: boot::image_handle(), controller: None };
    // SAFETY: GetProtocol is a non-exclusive read, so the firmware keeps ownership.
    let proto = unsafe { boot::open_protocol::<P>(params, OpenProtocolAttributes::GetProtocol) }?;
    Ok(Held::new(proto))
}

/// CloseProtocol, discarding the status. NOT_FOUND means the open record is
/// already gone; INVALID_PARAMETER means the handle itself was destroyed.
fn close(params: OpenProtocolParams, protocol: &Guid) {
    let Some(st) = uefi::table::system_table_raw() else {
        return;
    };
    // SAFETY: the system table and its boot services are live until exit.
    unsafe {
        let bt = (*st.as_ptr()).boot_services;
        let _ = ((*bt).close_protocol)(
            params.handle.as_ptr(),
            protocol,
            params.agent.as_ptr(),
            params.controller.map_or(ptr::null_mut(), |h| h.as_ptr()),
        );
    }
}
