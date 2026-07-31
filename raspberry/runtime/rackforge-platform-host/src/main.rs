use anyhow::{Result, bail};
use rackforge_platform_host::{detect_platform, serve};
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    match env::args().nth(1).as_deref() {
        Some("detect") => {
            let identity = detect_platform()?;
            println!("{}", serde_json::to_string_pretty(&identity)?);
            Ok(())
        }
        Some("serve") => {
            let socket = env::args()
                .nth(2)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/run/rackforge/platform-control.sock"));
            serve(&socket)
        }
        _ => bail!("usage: rackforge-platform-host detect|serve [SOCKET]"),
    }
}
