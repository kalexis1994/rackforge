use anyhow::{Context, Result};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

const INSTANCE_MUTEX_NAME: &str = "Global\\RackForge.Desktop.SingleInstance.v1";

pub(crate) enum AcquireOutcome {
    Acquired(SingleInstanceGuard),
    AlreadyRunning,
}

pub(crate) struct SingleInstanceGuard {
    handle: HANDLE,
}

pub(crate) fn acquire() -> Result<AcquireOutcome> {
    acquire_named(INSTANCE_MUTEX_NAME)
}

fn acquire_named(name: &str) -> Result<AcquireOutcome> {
    let name = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: the name is NUL-terminated and no security descriptor is
    // supplied. Windows owns the returned handle until it is closed.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error())
            .context("creating the RackForge single-instance mutex");
    }
    // SAFETY: GetLastError has no preconditions and CreateMutexW documents
    // ERROR_ALREADY_EXISTS as the signal that the named mutex already existed.
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already_exists {
        // SAFETY: handle was returned by CreateMutexW and is no longer needed.
        unsafe {
            CloseHandle(handle);
        }
        Ok(AcquireOutcome::AlreadyRunning)
    } else {
        Ok(AcquireOutcome::Acquired(SingleInstanceGuard { handle }))
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns the valid CreateMutexW handle.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_instance_is_rejected_and_a_closed_one_can_restart() {
        let name = format!(
            "Global\\RackForge.Desktop.SingleInstance.Test.{}",
            std::process::id()
        );
        let first = match acquire_named(&name).unwrap() {
            AcquireOutcome::Acquired(guard) => guard,
            AcquireOutcome::AlreadyRunning => panic!("unique test mutex already existed"),
        };
        assert!(matches!(
            acquire_named(&name).unwrap(),
            AcquireOutcome::AlreadyRunning
        ));
        drop(first);
        assert!(matches!(
            acquire_named(&name).unwrap(),
            AcquireOutcome::Acquired(_)
        ));
    }
}
