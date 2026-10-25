fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .set("ProductName", "声垣 SoundGuard")
            .set("FileDescription", "声垣原生声音监测采集端")
            .compile()
            .expect("compile Windows application icon and version");
    }
}
