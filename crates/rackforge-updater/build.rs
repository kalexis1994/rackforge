fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    let revision = std::env::var("GITHUB_SHA")
        .ok()
        .or_else(|| {
            std::process::Command::new("git")
                .args(["describe", "--always", "--dirty", "--abbrev=12"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().into())
        })
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=RACKFORGE_REVISION={revision}");
}
