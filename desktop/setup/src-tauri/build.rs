use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=SOTTO_SETUP_PAYLOAD");
    println!("cargo:rerun-if-changed=../../package.json");
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read("../../package.json").expect("desktop/package.json"))
            .unwrap();
    println!(
        "cargo:rustc-env=SOTTO_APP_VERSION={}",
        package["version"].as_str().unwrap()
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("payload.exe");
    let bytes = match env::var_os("SOTTO_SETUP_PAYLOAD") {
        Some(path) => {
            let path = PathBuf::from(path).canonicalize().expect("payload path");
            println!("cargo:rerun-if-changed={}", path.display());
            let bytes = fs::read(path).expect("read NSIS payload");
            assert!(
                bytes.starts_with(b"MZ"),
                "payload must be a Windows executable"
            );
            let capability: Vec<u8> = "SottoSetupOptionsV1"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            assert!(
                bytes
                    .windows(capability.len())
                    .any(|window| window == capability),
                "NSIS payload does not support setup options; rebuild it from current source"
            );
            bytes
        }
        None => {
            assert_ne!(
                env::var("PROFILE").unwrap(),
                "release",
                "release setup requires SOTTO_SETUP_PAYLOAD"
            );
            Vec::new()
        }
    };
    println!(
        "cargo:rustc-env=SOTTO_PAYLOAD_SHA256={:x}",
        Sha256::digest(&bytes)
    );
    fs::write(output, bytes).expect("stage embedded payload");
    tauri_build::build();
}
