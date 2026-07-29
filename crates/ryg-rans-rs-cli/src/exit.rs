//! # Stable exit codes for the ryg-rans CLI
//!
//! All exit codes are stable once documented.  Changing an exit code is a
//! breaking change for automation consumers.

/// Exit code constants.
///
/// | Code | Meaning |
/// |------|---------|
/// | 0 | Success |
/// | 2 | Command-line usage error (Clap) |
/// | 3 | Input/output error |
/// | 4 | Container or model format error |
/// | 5 | Integrity verification failure |
/// | 6 | Unsupported codec or format version |
/// | 7 | Resource limit exceeded |
/// | 8 | Parity or comparison mismatch |
/// | 9 | Requested backend unavailable |
/// | 10 | Internal invariant failure |
pub mod codes {
    /// Success.
    pub const SUCCESS: i32 = 0;
    /// Command-line usage error (handled by Clap).
    pub const USAGE: i32 = 2;
    /// Input/output error.
    pub const IO_ERROR: i32 = 3;
    /// Container or model format error.
    pub const FORMAT_ERROR: i32 = 4;
    /// Integrity verification failure.
    pub const INTEGRITY_ERROR: i32 = 5;
    /// Unsupported codec or format version.
    pub const UNSUPPORTED: i32 = 6;
    /// Resource limit exceeded.
    pub const RESOURCE_LIMIT: i32 = 7;
    /// Parity or comparison mismatch.
    pub const COMPARISON_MISMATCH: i32 = 8;
    /// Requested backend unavailable.
    pub const BACKEND_UNAVAILABLE: i32 = 9;
    /// Internal invariant failure.
    pub const INTERNAL_ERROR: i32 = 10;
}

/// Convert an error reference to a stable exit code.
pub fn error_to_exit_code(error: &crate::error::AppError) -> i32 {
    match error {
        crate::error::AppError::Io(_) => codes::IO_ERROR,
        crate::error::AppError::Format(_) => codes::FORMAT_ERROR,
        crate::error::AppError::Model(_) => codes::FORMAT_ERROR,
        crate::error::AppError::Codec(_) => codes::FORMAT_ERROR,
        crate::error::AppError::Integrity(_) => codes::INTEGRITY_ERROR,
        crate::error::AppError::ResourceLimit(_) => codes::RESOURCE_LIMIT,
        crate::error::AppError::Backend(_) => codes::BACKEND_UNAVAILABLE,
        crate::error::AppError::Comparison(_) => codes::COMPARISON_MISMATCH,
        crate::error::AppError::ExternalOracle(_) => codes::IO_ERROR,
        crate::error::AppError::InternalInvariant(_) => codes::INTERNAL_ERROR,
    }
}
