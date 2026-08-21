use core::fmt;
use std::process::Command;

const API_PACKAGE: &str = "game-api";

fn main() {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .expect("failed to run cargo metadata");

    if !output.status.success() {
        panic!("cargo metadata failed");
    }

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("invalid cargo metadata output");

    let packages = metadata["packages"].as_array().expect("missing packages");

    let game_api_pkg = packages
        .iter()
        .find(|package| package["name"].as_str() == Some(API_PACKAGE))
        .unwrap_or_else(|| {
            panic!(
                "package {API_PACKAGE} not found. found: {}",
                fmt::from_fn(|f| {
                    let mut iter = packages.iter().filter_map(|pkg| pkg["name"].as_str());
                    if let Some(first) = iter.next() {
                        f.write_str(first)?;
                    }
                    for package in iter {
                        f.write_str(", ")?;
                        f.write_str(package)?;
                    }
                    Ok(())
                })
            )
        });

    let version = game_api_pkg["version"]
        .as_str()
        .unwrap_or_else(|| panic!("{API_PACKAGE} has no version"));

    println!("cargo:rustc-env=API_VERSION={version}");
}
