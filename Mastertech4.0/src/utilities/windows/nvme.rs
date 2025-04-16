use std::alloc::Layout;

use nvme::Allocator;

pub struct NvmeAllocator;

impl nvme::Allocator for NvmeAllocator {
    unsafe fn allocate(&self, size: usize) -> usize {
        DmaManager::allocate(size)
    }

    unsafe fn deallocate(&self, addr: usize) {
        DmaManager::deallocate(addr);
    }

    fn translate(&self, addr: usize) -> usize {
        DmaManager::translate_addr(addr)
    }
}

pub fn get_nvme_info() -> anyhow::Result<(), anyhow::Error> {

    // Init the NVMe controller
    let controller = nvme::Device::init(virtual_address, nvme::Allocator)?;

    // Some useful data you may want to see
    let _controller_data = controller.controller_data();

    log::info!("Controller Data: {:?}", _controller_data);


    Ok(())
}