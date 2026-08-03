//! Benchmark metadata collection.
//!
//! # Post-v0.5.0 audit normalization (performance evidence)
//!
//! The evidence records distinguish three *different facts* that were
//! previously conflated:
//!
//! * **compiled target** — what the benchmark binary was compiled with:
//!   `target_cpu` (parsed from `RUSTFLAGS -C target-cpu=...`, `"default"`
//!   when absent), `codegen_flags` (the exact `RUSTFLAGS`), and
//!   `enabled_target_features` (the `#[cfg(target_feature = ...)]` set of
//!   the *exporting* process — approximately the benchmark binary's in the
//!   standard `benchmark-run` → `performance-seal` workflow, which runs
//!   both under the same `RUSTFLAGS`; the authoritative codegen facts are
//!   the flags themselves, which are bound to the benchmark run's
//!   `host.json`, not to the seal run).
//! * **runtime CPU** — what the host CPU can actually do, detected with
//!   `std::is_x86_feature_detected!()` at export time.
//! * **profile applicability** — `profile = "not_applicable"` when a
//!   benchmark case has no model-profile dimension (the model_cache
//!   surfaces), instead of the previous `"unknown"`.

use std::collections::HashMap;

/// Codegen facts about the benchmark build (audit normalization, see the
/// module docs).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompiledTargetInfo {
    /// `-C target-cpu=...` parsed from the benchmark run's `RUSTFLAGS`;
    /// `"default"` when the flag is absent.
    pub target_cpu: String,
    /// `#[cfg(target_feature = ...)]` enabled in the *exporting* process
    /// (the exporter runs under the same RUSTFLAGS as the benchmark in the
    /// standard workflow; `codegen_flags` is the authoritative fact).
    pub enabled_target_features: Vec<String>,
    /// The exact `RUSTFLAGS` of the benchmark run (bound to the run's
    /// `host.json`, never to the seal invocation's environment).
    pub codegen_flags: String,
}

impl Default for CompiledTargetInfo {
    fn default() -> Self {
        Self {
            target_cpu: "default".to_string(),
            enabled_target_features: Vec::new(),
            codegen_flags: String::new(),
        }
    }
}

/// Runtime CPU capabilities detected at export time.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeCpuInfo {
    /// Features the host CPU supports, detected via
    /// `std::is_x86_feature_detected!()`.
    pub detected_features: Vec<String>,
}

impl Default for RuntimeCpuInfo {
    fn default() -> Self {
        Self {
            detected_features: Vec::new(),
        }
    }
}

/// Benchmark host metadata.
pub struct BenchMetadata {
    pub rustc_version: String,
    /// Compile-time enabled target features (`#[cfg(target_feature = ...)]`)
    /// of the collecting process.  See [`CompiledTargetInfo`] for how this
    /// relates to the benchmark binary's actual codegen.
    pub target_features: Vec<String>,
    pub cpu_model: String,
    pub os_info: String,
    pub git_commit: String,
    pub dirty_tree: bool,
    pub num_cpus: usize,
    /// `-C target-cpu=...` from `RUSTFLAGS`, or `"default"`.
    pub target_cpu: String,
    /// The exact `RUSTFLAGS` of the collecting process.
    pub codegen_flags: String,
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

        let codegen_flags = std::env::var("RUSTFLAGS").unwrap_or_default();
        let target_cpu = parse_target_cpu(&codegen_flags);

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
            target_cpu,
            codegen_flags,
        }
    }

    /// Build the typed compiled-target facts from this metadata.
    pub fn compiled_target(&self) -> CompiledTargetInfo {
        CompiledTargetInfo {
            target_cpu: self.target_cpu.clone(),
            enabled_target_features: self.target_features.clone(),
            codegen_flags: self.codegen_flags.clone(),
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
        m.insert("target_cpu".to_string(), self.target_cpu.clone());
        m.insert("codegen_flags".to_string(), self.codegen_flags.clone());
        m
    }
}

/// Parse `-C target-cpu=NAME` out of a RUSTFLAGS string; `"default"` when
/// absent.  The first occurrence wins (rustc uses the last occurrence, but
/// a repeated flag is a pathological configuration; recording the first is
/// deterministic and the raw flags are preserved verbatim in
/// `codegen_flags` for the authoritative answer).
pub fn parse_target_cpu(rustflags: &str) -> String {
    for flag in rustflags.split_whitespace() {
        if let Some(cpu) = flag.strip_prefix("-Ctarget-cpu=") {
            return cpu.to_string();
        }
        if let Some(cpu) = flag.strip_prefix("-Ctarget-cpu:") {
            return cpu.to_string();
        }
        // Split-form `-C target-cpu=native` (two tokens).
    }
    let mut tokens = rustflags.split_whitespace();
    while let Some(t) = tokens.next() {
        if t == "-C" {
            if let Some(rest) = tokens.next() {
                if let Some(cpu) = rest.strip_prefix("target-cpu=") {
                    return cpu.to_string();
                }
            }
        }
    }
    "default".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_cpu_handles_all_flag_forms() {
        // Joined form.
        assert_eq!(parse_target_cpu("-Ctarget-cpu=native"), "native");
        // Split form.
        assert_eq!(parse_target_cpu("-C target-cpu=x86-64-v3"), "x86-64-v3");
        // Unrelated flags first.
        assert_eq!(
            parse_target_cpu("-C opt-level=3 -C target-cpu=native"),
            "native"
        );
        // Absent → default.
        assert_eq!(parse_target_cpu(""), "default");
        assert_eq!(parse_target_cpu("-C opt-level=3"), "default");
        assert_eq!(parse_target_cpu("-C target-feature=+avx2"), "default");
    }

    #[test]
    fn compiled_target_defaults_are_honest() {
        let d = CompiledTargetInfo::default();
        assert_eq!(d.target_cpu, "default");
        assert!(d.enabled_target_features.is_empty());
        assert!(d.codegen_flags.is_empty());
    }
}
