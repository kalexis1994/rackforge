// Stamps the git revision into the binary. In-repo builds ask git; trees
// produced by `git archive` carry the hash substituted into REVISION via
// export-subst (that is how the Raspberry Pi builds); anything else is dev.
fn main() {
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .or_else(|| {
            std::fs::read_to_string("../../REVISION")
                .ok()
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.contains("Format"))
        })
        .unwrap_or_else(|| "dev".to_owned());
    println!("cargo:rustc-env=RACKFORGE_REVISION={revision}");
    println!("cargo:rerun-if-changed=../../REVISION");
}
