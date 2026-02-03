//! OVATool Core Library
//!
//! This crate provides the core functionality for converting VMware VMs to OVA format.
//!
//! # Modules
//!
//! - `error` - Error types and Result alias
//! - `vmx` - VMX file parsing
//! - `vmdk` - VMDK disk handling
//! - `ovf` - OVF descriptor generation
//! - `ova` - OVA archive creation
//! - `pipeline` - Orchestration of the export process

pub mod error;
pub mod ova;
pub mod ovf;
pub mod pipeline;
pub mod vmdk;
pub mod vmx;

pub use error::{Error, Result};
