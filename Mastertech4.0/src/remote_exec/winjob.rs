//! Win32 job object wrapper so cancelling a RemoteExec job kills the whole
//! process tree, not just the interpreter.
//!
//! A PowerShell script that launches an installer leaves that installer running
//! if only the shell is terminated. Assigning the child to a job object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the kernel reap the tree.

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, TerminateJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct JobObject {
        handle: Option<HANDLE>,
    }

    // The handle is owned solely by this wrapper and only passed to Win32 calls
    // that are themselves thread-safe.
    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}

    impl JobObject {
        pub fn create() -> Self {
            let handle = unsafe { CreateJobObjectW(None, None) }.ok();
            if let Some(h) = handle {
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = unsafe {
                    SetInformationJobObject(
                        h,
                        JobObjectExtendedLimitInformation,
                        &info as *const _ as *const core::ffi::c_void,
                        std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    )
                };
                if ok.is_err() {
                    log::warn!("[remote_exec] SetInformationJobObject failed; tree kill degraded");
                }
            } else {
                log::warn!("[remote_exec] CreateJobObject failed; tree kill unavailable");
            }
            Self { handle }
        }

        /// Assigns a spawned process to this job. Returns false when the job
        /// object is unavailable or the process already belongs to a job that
        /// forbids nesting.
        pub fn assign(&self, raw_handle: isize) -> bool {
            let Some(job) = self.handle else { return false };
            let res = unsafe { AssignProcessToJobObject(job, HANDLE(raw_handle as *mut _)) };
            if let Err(e) = &res {
                log::warn!("[remote_exec] AssignProcessToJobObject failed: {e}");
            }
            res.is_ok()
        }

        /// Terminates every process in the job.
        pub fn terminate(&self) {
            if let Some(job) = self.handle {
                let _ = unsafe { TerminateJobObject(job, 1) };
            }
        }

        pub fn available(&self) -> bool {
            self.handle.is_some()
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            if let Some(h) = self.handle.take() {
                let _ = unsafe { CloseHandle(h) };
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub struct JobObject;

    impl JobObject {
        pub fn create() -> Self {
            Self
        }
        pub fn assign(&self, _raw_handle: isize) -> bool {
            false
        }
        pub fn terminate(&self) {}
        pub fn available(&self) -> bool {
            false
        }
    }
}

pub use imp::JobObject;
