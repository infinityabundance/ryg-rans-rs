//! Benchmark metadata collection.

use std::collections::HashMap;

/// Benchmark host metadata.
pub struct BenchMetadata {
    pub rustc_version: String,
    pub target_features: Vec<String>,
    pub cpu_model: String,
    pub os_info: String,
    pub git_commit: String,
    pub dirty_tree: bool,
    pub num_cpus: usize,
}

impl BenchMetadata {
    pub fn collect() -> Self {
        let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("model name"))
                    .map(|l| l.split(':').nth(1).unwrap_or("unknown").trim().to_string())
            })
            .unwrap_or_else(|| std::env::consts::ARCH.to_string());

        let os_info = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);

        let git_commit = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        let dirty_tree = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .ok()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);

        // Compiled target features.  `mut` is only exercised in native builds
        // (each push is cfg-gated); `cfg_attr` keeps the default build free
        // of an unused-mut warning while preserving the native build.
        #[cfg_attr(
            not(any(
                target_feature = "avx2",
                target_feature = "avx512f",
                target_feature = "avx512bw",
                target_feature = "avx512vl",
                target_feature = "sse4.1",
            )),
            allow(unused_mut)
        )]
        let mut features = Vec::new();
        #[cfg(target_feature = "avx2")]
        features.push("avx2".to_string());
        #[cfg(target_feature = "avx512f")]
        features.push("avx512f".to_string());
        #[cfg(target_feature = "avx512bw")]
        features.push("avx512bw".to_string());
        #[cfg(target_feature = "avx512vl")]
        features.push("avx512vl".to_string());
        #[cfg(target_feature = "sse4.1")]
        features.push("sse4.1".to_string());

        Self {
            rustc_version: std::process::Command::new("rustc")
                .args(["-vV"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_else(|| "unknown".to_string()),
            target_features: features,
            cpu_model,
            os_info,
            git_commit,
            dirty_tree,
            num_cpus: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
        }
    }

    pub fn to_map(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("rustc_version".to_string(), self.rustc_version.clone());
        m.insert(
            "target_features".to_string(),
            self.target_features.join(","),
        );
        m.insert("cpu_model".to_string(), self.cpu_model.clone());
        m.insert("os_info".to_string(), self.os_info.clone());
        m.insert("git_commit".to_string(), self.git_commit.clone());
        m.insert("dirty_tree".to_string(), self.dirty_tree.to_string());
        m.insert("num_cpus".to_string(), self.num_cpus.to_string());
        m
    }
}
