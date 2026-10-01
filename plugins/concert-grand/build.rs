fn main() {
    // The physical bridge and per-string loading enlarge the processor built
    // by value at initialization. Reserve stack space for this plugin only;
    // the workspace's other wasm components keep their existing stack size.
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo:rustc-link-arg-cdylib=-zstack-size=16777216");
    }
}
