use rustc_version::{Channel, version_meta};

fn main() {
    if let Ok(Channel::Nightly | Channel::Dev) = version_meta().map(|v| v.channel) {
        println!("cargo:rustc-cfg=nightly");
    }
    println!("cargo:rustc-check-cfg=cfg(nightly)");
}
