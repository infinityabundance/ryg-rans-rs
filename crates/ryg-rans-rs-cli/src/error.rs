//! # Typed errors for the ryg-rans CLI
//!
//! Every error carries structured context (path, block index, declared length,
//! limit, hash, etc.) without exposing untrusted payloads in error messages.
//!
//! Errors never include entire untrusted inputs.  They include identifying
//! metadata sufficient for debugging and triage.

// Re-export core error types for convenience.
pub use ryg_rans_rs_core::{DecodeError, EncodeError, ModelError};

use std::fmt;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// AppError — top-level error enum
// ---------------------------------------------------------------------------

/// Top-level error type for all CLI operations.
///
/// Each variant carries structured context relevant to the error kind.
/// The `Display` implementation produces user-facing messages.  The
/// `AppError::kind()` method returns a stable string identifier for
/// JSON serialization.
#[derive(Debug, Clone)]
pub enum AppError {
    /// I/O operation failed.
    Io(IoError),
    /// Container or stream format violation.
    Format(FormatError),
    /// Model construction or validation error.
    Model(ModelError),
    /// Codec operation failed.
    Codec(CodecError),
    /// SHA-256 integrity check failure.
    Integrity(IntegrityError),
    /// Resource limit exceeded.
    ResourceLimit(ResourceLimitError),
    /// Requested backend not available.
    Backend(BackendError),
    /// Unsupported codec or format version (stable exit code 6).
    Unsupported(UnsupportedError),
    /// Comparison mismatch.
    Comparison(ComparisonError),
    /// External oracle failure.
    ExternalOracle(OracleError),
    /// Internal invariant violation (bug).
    InternalInvariant(InternalInvariantError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Io(e) => write!(f, "I/O error: {}", e),
            AppError::Format(e) => write!(f, "format error: {}", e),
            AppError::Model(e) => write!(f, "model error: {}", e),
            AppError::Codec(e) => write!(f, "codec error: {}", e),
            AppError::Integrity(e) => write!(f, "integrity error: {}", e),
            AppError::ResourceLimit(e) => write!(f, "resource limit: {}", e),
            AppError::Backend(e) => write!(f, "backend error: {}", e),
            AppError::Unsupported(e) => write!(f, "unsupported: {}", e),
            AppError::Comparison(e) => write!(f, "comparison error: {}", e),
            AppError::ExternalOracle(e) => write!(f, "oracle error: {}", e),
            AppError::InternalInvariant(e) => write!(f, "internal error: {}", e),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Io(e) => Some(e),
            AppError::Format(e) => Some(e),
            AppError::Model(e) => Some(e),
            AppError::Codec(e) => Some(e),
            AppError::Integrity(e) => Some(e),
            AppError::ResourceLimit(e) => Some(e),
            AppError::Backend(e) => Some(e),
            AppError::Unsupported(e) => Some(e),
            AppError::Comparison(e) => Some(e),
            AppError::ExternalOracle(e) => Some(e),
            AppError::InternalInvariant(e) => Some(e),
        }
    }
}

impl AppError {
    /// Stable string identifier for JSON serialization.
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Io(_) => "io_error",
            AppError::Format(_) => "format_error",
            AppError::Model(_) => "model_error",
            AppError::Codec(_) => "codec_error",
            AppError::Integrity(_) => "integrity_failure",
            AppError::ResourceLimit(_) => "resource_limit",
            AppError::Backend(_) => "backend_unavailable",
            AppError::Unsupported(_) => "unsupported",
            AppError::Comparison(_) => "comparison_mismatch",
            AppError::ExternalOracle(_) => "oracle_error",
            AppError::InternalInvariant(_) => "internal_invariant",
        }
    }

    /// Return the stable exit code for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::Io(_) => 3,
            AppError::Format(_) => 4,
            AppError::Model(_) => 4,
            AppError::Codec(_) => 4,
            AppError::Integrity(_) => 5,
            AppError::ResourceLimit(_) => 7,
            AppError::Backend(_) => 9,
            AppError::Unsupported(_) => 6,
            AppError::Comparison(_) => 8,
            AppError::ExternalOracle(_) => 3,
            AppError::InternalInvariant(_) => 10,
        }
    }

    /// Convert to a JSON-compatible value.
    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind(),
            "message": self.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Concrete error types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct IoError {
    pub path: Option<PathBuf>,
    pub detail: String,
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(f, "{} (path: {})", self.detail, path.display())
        } else {
            write!(f, "{}", self.detail)
        }
    }
}

impl std::error::Error for IoError {}

#[derive(Debug, Clone)]
pub struct FormatError {
    pub detail: String,
    pub block_index: Option<u64>,
    pub offset: Option<u64>,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)?;
        if let Some(block) = self.block_index {
            write!(f, " (block {})", block)?;
        }
        if let Some(off) = self.offset {
            write!(f, " (offset {})", off)?;
        }
        Ok(())
    }
}

impl std::error::Error for FormatError {}

#[derive(Debug, Clone)]
pub struct CodecError {
    pub detail: String,
    pub codec_id: Option<u16>,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)?;
        if let Some(id) = self.codec_id {
            write!(f, " (codec {})", id)?;
        }
        Ok(())
    }
}

impl std::error::Error for CodecError {}

#[derive(Debug, Clone)]
pub struct IntegrityError {
    pub detail: String,
    pub block_index: Option<u64>,
}

impl fmt::Display for IntegrityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)?;
        if let Some(block) = self.block_index {
            write!(f, " (block {})", block)?;
        }
        Ok(())
    }
}

impl std::error::Error for IntegrityError {}

#[derive(Debug, Clone)]
pub struct ResourceLimitError {
    pub detail: String,
    pub limit: u64,
    pub requested: u64,
}

impl fmt::Display for ResourceLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (limit: {}, requested: {})",
            self.detail, self.limit, self.requested
        )
    }
}

impl std::error::Error for ResourceLimitError {}

#[derive(Debug, Clone)]
pub struct BackendError {
    pub detail: String,
    pub backend: String,
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (backend: {})", self.detail, self.backend)
    }
}

impl std::error::Error for BackendError {}

#[derive(Debug, Clone)]
pub struct UnsupportedError {
    pub detail: String,
}

impl fmt::Display for UnsupportedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for UnsupportedError {}

#[derive(Debug, Clone)]
pub struct ComparisonError {
    pub detail: String,
}

impl fmt::Display for ComparisonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for ComparisonError {}

#[derive(Debug, Clone)]
pub struct OracleError {
    pub detail: String,
    pub exit_code: Option<i32>,
}

impl fmt::Display for OracleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)?;
        if let Some(code) = self.exit_code {
            write!(f, " (exit: {})", code)?;
        }
        Ok(())
    }
}

impl std::error::Error for OracleError {}

#[derive(Debug, Clone)]
pub struct InternalInvariantError {
    pub detail: String,
}

impl fmt::Display for InternalInvariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "internal invariant: {}", self.detail)
    }
}

impl std::error::Error for InternalInvariantError {}

// ---------------------------------------------------------------------------
// From impls for core errors
// ---------------------------------------------------------------------------

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(IoError {
            path: None,
            detail: e.to_string(),
        })
    }
}

impl From<ryg_rans_rs_core::ModelError> for AppError {
    fn from(e: ryg_rans_rs_core::ModelError) -> Self {
        AppError::Model(e)
    }
}

impl From<ryg_rans_rs_core::EncodeError> for AppError {
    fn from(_e: ryg_rans_rs_core::EncodeError) -> Self {
        AppError::Codec(CodecError {
            detail: "encode buffer too small".into(),
            codec_id: None,
        })
    }
}

impl From<ryg_rans_rs_core::DecodeError> for AppError {
    fn from(_e: ryg_rans_rs_core::DecodeError) -> Self {
        AppError::Format(FormatError {
            detail: "truncated stream".into(),
            block_index: None,
            offset: None,
        })
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::InternalInvariant(InternalInvariantError { detail: s })
    }
}
