//! Resource snapshots are requested by settings, never by audio capture.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize)]
pub struct HardwareSnapshot {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub gpu_backend: &'static str,
}

pub fn gpu_backend() -> &'static str {
    if cfg!(feature = "gpu-vulkan") {
        "vulkan"
    } else if cfg!(feature = "gpu-metal") {
        "metal"
    } else {
        "none"
    }
}

pub fn snapshot() -> HardwareSnapshot {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    HardwareSnapshot {
        total_bytes: (system.total_memory() > 0).then_some(system.total_memory()),
        // Zero available RAM is valid. A fresh failed Windows snapshot has
        // zero total RAM as well, since both values come from one OS call.
        available_bytes: (system.total_memory() > 0).then_some(system.available_memory()),
        gpu_backend: gpu_backend(),
    }
}

/// No serials, hostnames or network identifiers. Kept only in the local DB.
pub fn fingerprint() -> &'static str {
    static VALUE: OnceLock<String> = OnceLock::new();
    VALUE.get_or_init(|| {
        let mut system = sysinfo::System::new();
        system.refresh_cpu_all();
        system.refresh_memory();
        let cpu = system.cpus().first().map(|cpu| cpu.brand()).unwrap_or("");
        let description = format!(
            "{}:{}:{cpu}:{}:{}:{:?}:{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            system.cpus().len(),
            system.total_memory(),
            sysinfo::System::os_version(),
            gpu_backend(),
        );
        format!("{:x}", Sha256::digest(description.as_bytes()))
    })
}

/// whisper-rs 0.14 does not expose which backend actually executed a graph.
pub fn compute(cpu_only: bool, requested_gpu: bool) -> &'static str {
    if cpu_only || !requested_gpu || gpu_backend() == "none" {
        "cpu"
    } else {
        "gpu_unverified"
    }
}
