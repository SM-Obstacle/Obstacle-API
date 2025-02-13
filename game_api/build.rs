fn main() {
    #[cfg(not(any(feature = "no_auth", test)))]
    {
        println!("cargo:rustc-cfg=auth");
        println!("cargo:rustc-check-cfg=cfg(auth)")
    }
}
