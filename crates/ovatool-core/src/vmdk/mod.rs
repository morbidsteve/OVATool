//! VMDK disk handling.
//!
//! This module provides functionality for reading and processing VMDK files,
//! including sparse disk formats and stream-optimized conversion.

pub mod descriptor;
pub mod reader;

pub use descriptor::{parse_descriptor, Extent, ExtentType, VmdkDescriptor};
pub use reader::{ChunkIterator, IndexedChunk, IndexedChunkIterator, VmdkReader};
