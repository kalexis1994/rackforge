use anyhow::{Context, Result, bail};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::Path;

#[derive(Debug)]
pub(crate) struct AudioEngineGuard {
    _file: File,
}

impl AudioEngineGuard {
    pub(crate) fn acquire(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .context("RackForge audio-engine lock has no parent directory")?;
        std::fs::create_dir_all(parent).with_context(|| {
            format!("creating audio-engine state directory {}", parent.display())
        })?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| format!("opening audio-engine lock {}", path.display()))?;
        // SAFETY: the descriptor belongs to the open File and the operation
        // does not outlive it. LOCK_NB ensures a second process never waits
        // while the first process owns the audio engine.
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN)
            {
                bail!(
                    "RackForge audio engine is already running; the second instance was not started"
                );
            }
            return Err(error).with_context(|| {
                format!("acquiring RackForge audio-engine lock {}", path.display())
            });
        }
        file.set_len(0)
            .with_context(|| format!("resetting audio-engine lock {}", path.display()))?;
        writeln!(file, "{}", std::process::id())
            .with_context(|| format!("recording audio-engine owner in {}", path.display()))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_rejects_a_second_engine_and_recovers_after_drop() {
        let directory = std::env::temp_dir().join(format!(
            "rackforge-audio-instance-test-{}",
            std::process::id()
        ));
        let path = directory.join("audio-engine.lock");
        let first = AudioEngineGuard::acquire(&path).unwrap();
        assert!(
            AudioEngineGuard::acquire(&path)
                .unwrap_err()
                .to_string()
                .contains("already running")
        );
        drop(first);
        drop(AudioEngineGuard::acquire(&path).unwrap());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
