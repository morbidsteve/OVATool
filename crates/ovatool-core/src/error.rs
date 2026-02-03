//! Error types for the OVATool core library.

use std::path::PathBuf;

/// The main error type for OVATool operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// I/O error with optional path context.
    #[error("I/O error{}: {source}", path.as_ref().map(|p| format!(" at '{}'", p.display())).unwrap_or_default())]
    Io {
        source: std::io::Error,
        path: Option<PathBuf>,
    },

    /// Error parsing a VMX file.
    #[error("VMX parse error: {message}")]
    VmxParse { message: String },

    /// Error reading or processing a VMDK file.
    #[error("VMDK error: {message}")]
    Vmdk { message: String },

    /// Error generating OVF descriptor.
    #[error("OVF error: {message}")]
    Ovf { message: String },

    /// Error creating OVA archive.
    #[error("OVA error: {message}")]
    Ova { message: String },

    /// Error in the export pipeline.
    #[error("Pipeline error: {message}")]
    Pipeline { message: String },
}

/// A specialized Result type for OVATool operations.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Create an I/O error with path context.
    pub fn io(source: std::io::Error, path: impl Into<PathBuf>) -> Self {
        Self::Io {
            source,
            path: Some(path.into()),
        }
    }

    /// Create an I/O error without path context.
    pub fn io_simple(source: std::io::Error) -> Self {
        Self::Io { source, path: None }
    }

    /// Create a VMX parse error.
    pub fn vmx_parse(message: impl Into<String>) -> Self {
        Self::VmxParse {
            message: message.into(),
        }
    }

    /// Create a VMDK error.
    pub fn vmdk(message: impl Into<String>) -> Self {
        Self::Vmdk {
            message: message.into(),
        }
    }

    /// Create an OVF error.
    pub fn ovf(message: impl Into<String>) -> Self {
        Self::Ovf {
            message: message.into(),
        }
    }

    /// Create an OVA error.
    pub fn ova(message: impl Into<String>) -> Self {
        Self::Ova {
            message: message.into(),
        }
    }

    /// Create a pipeline error.
    pub fn pipeline(message: impl Into<String>) -> Self {
        Self::Pipeline {
            message: message.into(),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Self::io_simple(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_with_path() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = Error::io(io_err, "/path/to/file.vmx");
        let msg = err.to_string();
        assert!(msg.contains("I/O error"));
        assert!(msg.contains("/path/to/file.vmx"));
    }

    #[test]
    fn test_io_error_without_path() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = Error::io_simple(io_err);
        let msg = err.to_string();
        assert!(msg.contains("I/O error"));
        assert!(!msg.contains("at '"));
    }

    #[test]
    fn test_vmx_parse_error() {
        let err = Error::vmx_parse("invalid syntax");
        assert!(err.to_string().contains("VMX parse error"));
        assert!(err.to_string().contains("invalid syntax"));
    }

    #[test]
    fn test_vmdk_error() {
        let err = Error::vmdk("unsupported format");
        assert!(err.to_string().contains("VMDK error"));
    }

    #[test]
    fn test_ovf_error() {
        let err = Error::ovf("invalid XML");
        assert!(err.to_string().contains("OVF error"));
    }

    #[test]
    fn test_ova_error() {
        let err = Error::ova("tar creation failed");
        assert!(err.to_string().contains("OVA error"));
    }

    #[test]
    fn test_pipeline_error() {
        let err = Error::pipeline("worker thread panicked");
        assert!(err.to_string().contains("Pipeline error"));
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io { path: None, .. }));
    }
}
