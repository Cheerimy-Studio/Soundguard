fn main() {
    if std::env::var("TARGET").is_ok_and(|target| target.contains("win7")) {
        let libs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("win7-libs");
        println!("cargo:rustc-link-search=native={}", libs.display());
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .set("ProductName", "声垣")
            .set("FileDescription", "声垣")
            .compile()
            .expect("compile Windows application icon and version");
    }
}
