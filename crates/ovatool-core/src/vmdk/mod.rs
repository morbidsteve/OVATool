//! VMDK disk handling.
//!
//! This module provides functionality for reading and processing VMDK files,
//! including sparse disk formats and stream-optimized conversion.

pub mod descriptor;
pub mod reader;
pub mod stream;

pub use descriptor::{parse_descriptor, Extent, ExtentType, VmdkDescriptor};
pub use reader::{ChunkIterator, IndexedChunk, IndexedChunkIterator, VmdkReader};
pub use stream::{
    compress_grain, GrainMarker, Marker, MarkerType, SparseExtentHeader, StreamVmdkWriter,
    DEFAULT_GRAIN_SIZE, GT_ENTRIES_PER_GT, SECTOR_SIZE, VMDK_MAGIC,
};
