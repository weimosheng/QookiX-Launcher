fn main() {
    println!("cargo:rerun-if-changed=installer-ui.rc");
    println!("cargo:rerun-if-changed=../icons/icon.ico");
    println!("cargo:rerun-if-changed=../icons/128x128.png");
    embed_resource::compile("installer-ui.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("failed to embed the installer icon");
}
