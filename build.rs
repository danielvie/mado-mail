fn main() {
    println!("cargo:rerun-if-changed=build/app.rc");
    println!("cargo:rerun-if-changed=build/icon.ico");
    #[cfg(target_os = "windows")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("build/app.rc", embed_resource::NONE)
            .manifest_required()
            .expect("Could not embed the Mado Mail application icon");
    }
}
