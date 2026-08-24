use std::{
    fs::{OpenOptions, create_dir_all},
    io::Write,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

static LOG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

pub fn write(message: impl AsRef<str>) {
    let path = LOG_PATH.get_or_init(|| {
        let directory = std::env::temp_dir().join("RackForge");
        let _ = create_dir_all(&directory);
        directory.join("rackforge-vst3.log")
    });
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let _ = writeln!(
        file,
        "{millis} pid={} {}",
        std::process::id(),
        message.as_ref()
    );
}
