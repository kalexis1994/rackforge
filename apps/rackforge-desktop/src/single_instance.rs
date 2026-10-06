use anyhow::{Context, Result};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::CreateMutexW;

#[cfg(windows)]
const INSTANCE_MUTEX_NAME: &str = "Global\\RackForge.Desktop.SingleInstance.v1";

pub(crate) enum AcquireOutcome {
    Acquired(SingleInstanceGuard),
    AlreadyRunning,
}

#[cfg(windows)]
pub(crate) struct SingleInstanceGuard {
    handle: HANDLE,
}

/// On Linux, an exclusive lock on a file in the session's runtime directory:
/// the kernel drops it when the process ends, however it ends, so a crash
/// never leaves a stale guard behind. Inside a Flatpak the runtime directory
/// is the app's own.
#[cfg(not(windows))]
pub(crate) struct SingleInstanceGuard {
    _file: std::fs::File,
}

#[cfg(windows)]
pub(crate) fn acquire() -> Result<AcquireOutcome> {
    acquire_named(INSTANCE_MUTEX_NAME)
}

#[cfg(not(windows))]
pub(crate) fn acquire() -> Result<AcquireOutcome> {
    let directory = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    acquire_file(&directory.join("rackforge-desktop.lock"))
}

#[cfg(windows)]
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

#[cfg(not(windows))]
fn acquire_file(path: &std::path::Path) -> Result<AcquireOutcome> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .with_context(|| format!("opening the single-instance lock {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(AcquireOutcome::Acquired(SingleInstanceGuard {
            _file: file,
        })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(AcquireOutcome::AlreadyRunning),
        Err(std::fs::TryLockError::Error(error)) => Err(error)
            .with_context(|| format!("locking the single-instance lock {}", path.display())),
    }
}

#[cfg(windows)]
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

    #[cfg(windows)]
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

    #[cfg(not(windows))]
    #[test]
    fn a_second_instance_is_rejected_and_a_closed_one_can_restart() {
        let path = std::env::temp_dir().join(format!(
            "rackforge-desktop-single-instance-test-{}.lock",
            std::process::id()
        ));
        let first = match acquire_file(&path).unwrap() {
            AcquireOutcome::Acquired(guard) => guard,
            AcquireOutcome::AlreadyRunning => panic!("unique test lock already held"),
        };
        assert!(matches!(
            acquire_file(&path).unwrap(),
            AcquireOutcome::AlreadyRunning
        ));
        drop(first);
        assert!(matches!(
            acquire_file(&path).unwrap(),
            AcquireOutcome::Acquired(_)
        ));
        let _ = std::fs::remove_file(path);
    }
}
