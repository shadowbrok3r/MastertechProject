//! Bugcheck code → name table and per-code parameter interpretation.

/// Human name for a Windows bugcheck code.
pub fn bugcheck_name(code: u32) -> &'static str {
    match code {
        0x01 => "APC_INDEX_MISMATCH",
        0x0A => "IRQL_NOT_LESS_OR_EQUAL",
        0x1A => "MEMORY_MANAGEMENT",
        0x1E => "KMODE_EXCEPTION_NOT_HANDLED",
        0x21 => "QUOTA_UNDERFLOW",
        0x24 => "NTFS_FILE_SYSTEM",
        0x2E => "DATA_BUS_ERROR",
        0x3B => "SYSTEM_SERVICE_EXCEPTION",
        0x3D => "INTERRUPT_EXCEPTION_NOT_HANDLED",
        0x41 => "MUST_SUCCEED_POOL_EMPTY",
        0x44 => "MULTIPLE_IRP_COMPLETE_REQUESTS",
        0x4E => "PFN_LIST_CORRUPT",
        0x50 => "PAGE_FAULT_IN_NONPAGED_AREA",
        0x51 => "REGISTRY_ERROR",
        0x5A => "CRITICAL_SERVICE_FAILED",
        0x65 => "MEMORY1_INITIALIZATION_FAILED",
        0x74 => "BAD_SYSTEM_CONFIG_INFO",
        0x77 => "KERNEL_STACK_INPAGE_ERROR",
        0x7A => "KERNEL_DATA_INPAGE_ERROR",
        0x7E => "SYSTEM_THREAD_EXCEPTION_NOT_HANDLED",
        0x7F => "UNEXPECTED_KERNEL_MODE_TRAP",
        0x80 => "NMI_HARDWARE_FAILURE",
        0x8E => "KERNEL_MODE_EXCEPTION_NOT_HANDLED",
        0x9C => "MACHINE_CHECK_EXCEPTION",
        0x9F => "DRIVER_POWER_STATE_FAILURE",
        0xA0 => "INTERNAL_POWER_ERROR",
        0xAB => "SESSION_HAS_VALID_POOL_ON_EXIT",
        0xBE => "ATTEMPTED_WRITE_TO_READONLY_MEMORY",
        0xC1 => "SPECIAL_POOL_DETECTED_MEMORY_CORRUPTION",
        0xC2 => "BAD_POOL_CALLER",
        0xC4 => "DRIVER_VERIFIER_DETECTED_VIOLATION",
        0xC5 => "DRIVER_CORRUPTED_EXPOOL",
        0xC9 => "DRIVER_VERIFIER_IOMANAGER_VIOLATION",
        0xCE => "DRIVER_UNLOADED_WITHOUT_CANCELLING_PENDING_OPERATIONS",
        0xD1 => "DRIVER_IRQL_NOT_LESS_OR_EQUAL",
        0xD5 => "DRIVER_PAGE_FAULT_IN_FREED_SPECIAL_POOL",
        0xDE => "POOL_CORRUPTION_IN_FILE_AREA",
        0xE1 => "WORKER_THREAD_RETURNED_AT_BAD_IRQL",
        0xE2 => "MANUALLY_INITIATED_CRASH",
        0xE3 => "RESOURCE_NOT_OWNED",
        0xE4 => "WORKER_INVALID",
        0xEF => "CRITICAL_PROCESS_DIED",
        0xF4 => "CRITICAL_OBJECT_TERMINATION",
        0xF5 => "FLTMGR_FILE_SYSTEM",
        0xF7 => "DRIVER_OVERRAN_STACK_BUFFER",
        0xFC => "ATTEMPTED_EXECUTE_OF_NOEXECUTE_MEMORY",
        0xFE => "BUGCODE_USB_DRIVER",
        0x109 => "CRITICAL_STRUCTURE_CORRUPTION",
        0x116 => "VIDEO_TDR_FAILURE",
        0x117 => "VIDEO_TDR_TIMEOUT_DETECTED",
        0x119 => "VIDEO_SCHEDULER_INTERNAL_ERROR",
        0x124 => "WHEA_UNCORRECTABLE_ERROR",
        0x133 => "DPC_WATCHDOG_VIOLATION",
        0x139 => "KERNEL_SECURITY_CHECK_FAILURE",
        0x13A => "KERNEL_MODE_HEAP_CORRUPTION",
        0x141 => "VIDEO_ENGINE_TIMEOUT_DETECTED",
        0x142 => "VIDEO_TDR_APPLICATION_BLOCKED",
        0x154 => "UNEXPECTED_STORE_EXCEPTION",
        0x15F => "CONNECTED_STANDBY_WATCHDOG_TIMEOUT",
        0x162 => "KERNEL_AUTO_BOOST_INVALID_LOCK_RELEASE",
        0x18B => "CORRUPTED_SYSTEM_FILE",
        0x192 => "KERNEL_AUTO_BOOST_LOCK_ACQUISITION_WITH_RAISED_IRQL",
        0x1A0 => "TTM_FATAL_ERROR",
        0x1C4 => "DRIVER_VERIFIER_DETECTED_VIOLATION_LIVEDUMP",
        0x1C8 => "MANUALLY_INITIATED_CRASH1",
        0x1CA => "SYNTHETIC_WATCHDOG_TIMEOUT",
        0x1D3 => "WFP_INVALID_OPERATION",
        0x1E2 => "HARDWARE_WATCHDOG_TIMEOUT",
        0x356 => "XBOX_ERACTRL_CS_TIMEOUT",
        _ => "UNKNOWN_BUGCHECK",
    }
}

/// Short interpretation of the four bugcheck parameters for the common codes.
/// Returns one note per parameter that has a documented meaning.
pub fn parameter_notes(code: u32, p: &[u64; 4]) -> Vec<String> {
    match code {
        0x0A | 0xD1 => vec![
            format!("P1: memory referenced = {:#x}", p[0]),
            format!("P2: IRQL at fault = {}", p[1]),
            format!(
                "P3: {} operation",
                match p[2] {
                    0 => "read",
                    1 => "write",
                    8 => "execute",
                    _ => "unknown",
                }
            ),
            format!("P4: faulting address = {:#x}", p[3]),
        ],
        0x50 => vec![
            format!("P1: memory referenced = {:#x}", p[0]),
            format!(
                "P2: {} operation",
                match p[1] {
                    0 => "read",
                    1 => "write",
                    2 | 10 => "execute",
                    _ => "unknown",
                }
            ),
            format!("P3: faulting address = {:#x}", p[2]),
        ],
        0x1E | 0x7E | 0x8E => vec![
            format!("P1: exception code = {:#x}", p[0]),
            format!("P2: address where exception occurred = {:#x}", p[1]),
        ],
        0x3B => vec![
            format!("P1: exception code = {:#x}", p[0]),
            format!("P2: address of the instruction = {:#x}", p[1]),
            format!("P3: context record = {:#x}", p[2]),
        ],
        0x9F => vec![
            format!(
                "P1: {}",
                match p[0] {
                    3 => "device object blocked an IRP too long",
                    4 => "power state transition timed out",
                    _ => "see docs for this subtype",
                }
            ),
            format!("P2: device object / timeout = {:#x}", p[1]),
        ],
        0x116 | 0x117 => vec![
            format!("P1: TDR recovery context = {:#x}", p[0]),
            format!("P2: driver entrypoint = {:#x} (the display driver)", p[1]),
        ],
        0x124 => vec![
            format!(
                "P1: {}",
                match p[0] {
                    0 => "machine check exception (MCE)",
                    1 => "corrected machine check",
                    2 => "corrected platform error",
                    3 => "NMI error",
                    _ => "WHEA error source",
                }
            ),
            format!("P2: WHEA_ERROR_RECORD address = {:#x}", p[1]),
        ],
        0x133 => vec![
            format!(
                "P1: {}",
                match p[0] {
                    0 => "single DPC exceeded its time allotment",
                    1 => "system cumulatively spent too long at DISPATCH_LEVEL+",
                    _ => "watchdog subtype",
                }
            ),
            format!("P2: watchdog period (ticks) = {}", p[1]),
        ],
        0x139 => vec![
            format!(
                "P1: {}",
                match p[0] {
                    2 => "stack cookie corruption",
                    3 => "corrupted LIST_ENTRY",
                    4 => "out-of-range stack pointer",
                    _ => "security check subtype",
                }
            ),
            format!("P2: trap frame = {:#x}", p[1]),
        ],
        0x1A => vec![format!("P1: memory-management subtype = {:#x}", p[0])],
        0xEF => vec![format!("P1: process object = {:#x}", p[0])],
        _ => Vec::new(),
    }
}
