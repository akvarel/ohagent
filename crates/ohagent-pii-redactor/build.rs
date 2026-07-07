//! Build script: generates license signing secret.
//!
//! In CI/production: `OHAGENT_PII_SECRET` env var is set.
//! In dev: generates a random secret (no license validation).

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let secret_path = out_dir.join("license_secret.bin");

    let secret = if let Ok(s) = env::var("OHAGENT_PII_SECRET") {
        hex::decode(&s).expect("OHAGENT_PII_SECRET must be hex-encoded")
    } else {
        // Dev builds: use a single zero byte = no validation
        vec![0u8]
    };

    fs::write(&secret_path, &secret).expect("Failed to write license secret");
    println!("cargo:rerun-if-env-changed=OHAGENT_PII_SECRET");
}
