//! Writes Islet's app icon from the shapes in src/logo.rs and puts it in the exe.
#[allow(dead_code)]
#[path = "src/logo.rs"]
mod logo;

fn main() {
    println!("cargo:rerun-if-changed=src/logo.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let images: Vec<_> = logo::ICON_SIZES.iter().map(|&s| (s, logo::logo(s, true))).collect();
    let ico = out.join("islet.ico");
    std::fs::write(&ico, logo::ico(&images)).unwrap();
    let rc = out.join("islet.rc");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico.display().to_string().replace(std::path::MAIN_SEPARATOR, "/"))).unwrap();
    embed_resource::compile(&rc, embed_resource::NONE).manifest_optional().unwrap();
}
