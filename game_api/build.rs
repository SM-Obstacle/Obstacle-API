fn main() {
    #[cfg(all(feature = "auth", not(test)))]
    println!("cargo:rustc-cfg=auth");
    println!("cargo:rustc-check-cfg=cfg(auth)")
}
