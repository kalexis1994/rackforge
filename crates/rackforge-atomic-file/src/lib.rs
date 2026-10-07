//! Files written so that a power cut leaves the old content or the new,
//! never half of either.
//!
//! An instrument is switched off at the wall. Every file RackForge writes
//! while it runs -- a session, a program, a setting -- goes through
//! [`write`]: the bytes go to a temporary file beside the target, are forced
//! to the card, and then replace the target in one rename, after which the
//! directory itself is forced to the card so the rename survives too.
//!
//! A temporary file a power cut left behind is named so that [`is_temporary`]
//! recognises it: whoever reads a directory skips it, and [`remove_stale`]
//! clears it. A file that is damaged anyway -- by an older RackForge, or by
//! the card -- is moved aside by [`quarantine`] rather than allowed to stop
//! a program from starting.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The marker in every temporary file's name: `.{name}.rackforge-tmp-{pid}-{n}`.
const TEMPORARY_MARKER: &str = ".rackforge-tmp-";

/// The marker in the name of a file moved aside: `{name}.corrupt-{seconds}`.
const QUARANTINE_MARKER: &str = ".corrupt-";

static SERIAL: AtomicU64 = AtomicU64::new(0);

/// Replaces `path` with `bytes`, atomically and durably.
///
/// The parent directory must exist. On success the new content is on the
/// card; on failure, or after a power cut at any moment, `path` holds
/// either its old content or the new one.
pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    let path = path.as_ref();
    let (parent, name) = split(path)?;
    let temporary = parent.join(temporary_name(&name));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes.as_ref())?;
        file.sync_all()?;
        drop(file);
        // Rename replaces the target in one step; removing it first would
        // open a window with no file at all.
        fs::rename(&temporary, path)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Copies `source` over `destination` the same way [`write`] does.
pub fn copy(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> io::Result<()> {
    let bytes = fs::read(source)?;
    write(destination, bytes)
}

/// Forces a directory's entries -- a rename into it, a new file -- to the
/// card. A no-op where the platform cannot open a directory as a file.
pub fn sync_directory(directory: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(directory.as_ref())?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        Ok(())
    }
}

/// Whether `path` names a temporary file [`write`] makes: left behind only
/// by a power cut or a crash, and never a document.
pub fn is_temporary(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') && name.contains(TEMPORARY_MARKER))
}

/// Whether `path` names a file [`quarantine`] moved aside.
pub fn is_quarantined(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains(QUARANTINE_MARKER))
}

/// Removes the temporary files a power cut left in `directory`, and returns
/// how many. A directory that does not exist has none.
pub fn remove_stale(directory: impl AsRef<Path>) -> io::Result<usize> {
    let entries = match fs::read_dir(directory.as_ref()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut removed = 0;
    for entry in entries {
        let path = entry?.path();
        if is_temporary(&path) && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Moves a damaged file aside, beside itself, so that what reads the
/// directory starts without it, and the player or a developer can still
/// look at it. Returns where it went.
pub fn quarantine(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = path.as_ref();
    let (parent, name) = split(path)?;
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let mut moved = name;
    moved.push(format!("{QUARANTINE_MARKER}{seconds}"));
    let destination = parent.join(moved);
    fs::rename(path, &destination)?;
    sync_directory(parent)?;
    Ok(destination)
}

fn split(path: &Path) -> io::Result<(&Path, OsString)> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} names no file", path.display()),
        )
    })?;
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    Ok((parent, name.to_os_string()))
}

fn temporary_name(name: &OsString) -> OsString {
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(
        "{TEMPORARY_MARKER}{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    temporary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "rackforge-atomic-file-{name}-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn a_write_replaces_the_file_and_leaves_nothing_beside_it() {
        let directory = scratch("write");
        let path = directory.join("session.json");
        write(&path, b"old").unwrap();
        write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        let names: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![OsString::from("session.json")]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn what_a_power_cut_leaves_is_recognised_and_cleared() {
        let directory = scratch("stale");
        let left = directory.join(temporary_name(&OsString::from("rack.json")));
        fs::write(&left, b"half a rac").unwrap();
        fs::write(directory.join("rack.json"), b"{}").unwrap();
        assert!(is_temporary(&left));
        assert!(!is_temporary(directory.join("rack.json")));
        assert!(!is_temporary(directory.join(".hidden.json")));
        assert_eq!(remove_stale(&directory).unwrap(), 1);
        assert!(!left.exists());
        assert!(directory.join("rack.json").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_damaged_file_is_moved_aside_not_lost() {
        let directory = scratch("quarantine");
        let path = directory.join("audio.toml");
        fs::write(&path, b"not = [toml").unwrap();
        let moved = quarantine(&path).unwrap();
        assert!(!path.exists());
        assert!(is_quarantined(&moved));
        assert_eq!(fs::read(&moved).unwrap(), b"not = [toml");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_missing_directory_has_nothing_stale() {
        assert_eq!(remove_stale(scratch("gone").join("absent")).unwrap(), 0);
    }
}
