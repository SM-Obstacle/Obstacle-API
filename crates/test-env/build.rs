fn main() {
    println!("cargo:rustc-check-cfg=cfg(test_force_db_deletion)");
}
