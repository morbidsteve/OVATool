//! OVATool Core Library
//!
//! This crate provides the core functionality for converting VMware VMs to OVA format.
//!
//! # Overview
//!
//! OVATool converts VMware VMs (VMX + VMDK files) to OVA format for portability.
//! The main entry point is the [`export_vm`] function which handles the full conversion.
//!
//! # Modules
//!
//! - [`error`] - Error types and Result alias
//! - [`vmx`] - VMX file parsing
//! - [`vmdk`] - VMDK disk handling (reading, compression, stream-optimized writing)
//! - [`ovf`] - OVF descriptor generation
//! - [`ova`] - OVA archive creation
//! - [`pipeline`] - Parallel processing pipeline
//! - [`export`] - Export orchestrator coordinating the full pipeline
//!
//! # Quick Start
//!
//! ```no_run
//! use ovatool_core::{export_vm, ExportOptions};
//! use std::path::Path;
//!
//! let vmx_path = Path::new("/path/to/vm.vmx");
//! let output_path = Path::new("/path/to/output.ova");
//!
//! export_vm(vmx_path, output_path, ExportOptions::default(), None).unwrap();
//! ```

pub mod error;
pub mod export;
pub mod ova;
pub mod ovf;
pub mod pipeline;
pub mod vmdk;
pub mod vmx;

pub use error::{Error, Result};

// Re-export main export functionality for convenience
pub use export::{
    export_vm, get_vm_info, DiskDetail, ExportOptions, ExportPhase, ExportProgress,
    ProgressCallback, VmInfo, DEFAULT_CHUNK_SIZE,
};

// Re-export compression level from pipeline
pub use pipeline::CompressionLevel;
