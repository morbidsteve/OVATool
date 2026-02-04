//! StreamOptimized VMDK writer.
//!
//! This module provides functionality for creating VMware-compatible
//! streamOptimized VMDK files with deflate compression.
//!
//! StreamOptimized VMDKs are designed for efficient streaming and OVA packaging.
//! They use:
//! - Version 3 format (streamOptimized)
//! - Markers for grain tables, directories, and end-of-stream
//! - DEFLATE compression for grain data
//! - Footer with actual grain directory offset

use crate::error::{Error, Result};
use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::collections::BTreeMap;
use std::io::{Seek, Write};

/// VMDK magic number ("VMDK" as little-endian u32).
pub const VMDK_MAGIC: u32 = 0x564D444B;

/// Size of a sector in bytes.
pub const SECTOR_SIZE: u64 = 512;

/// Default grain size in sectors (128 sectors = 64KB).
pub const DEFAULT_GRAIN_SIZE: u64 = 128;

/// Number of grain table entries per grain table.
pub const GT_ENTRIES_PER_GT: u32 = 512;

/// Flags for streamOptimized VMDK.
/// - Bit 0: Valid new line detection
/// - Bit 16: Compressed grains
/// - Bit 17: Markers
const STREAM_OPTIMIZED_FLAGS: u32 = 0x30001 | (1 << 16) | (1 << 17);

/// Grain directory offset value indicating GD is at end of file.
const GD_AT_END: u64 = 0xFFFFFFFFFFFFFFFF;

/// Compression algorithm: DEFLATE.
const COMPRESS_ALGORITHM_DEFLATE: u16 = 1;

/// Marker types used in streamOptimized VMDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MarkerType {
    /// No marker / End of stream marker.
    EndOfStream = 0,
    /// Grain table marker.
    GrainTable = 1,
    /// Grain directory marker.
    GrainDirectory = 2,
    /// Footer marker.
    Footer = 3,
}

/// A marker structure used in streamOptimized VMDK.
///
/// Markers are 512-byte structures that precede metadata regions.
#[derive(Debug, Clone)]
pub struct Marker {
    /// Number of sectors that follow this marker (for GD/GT).
    pub num_sectors: u64,
    /// Size in bytes (for compressed grains).
    pub size: u32,
    /// Marker type.
    pub marker_type: MarkerType,
}

impl Marker {
    /// Creates a new marker.
    pub fn new(marker_type: MarkerType, num_sectors: u64) -> Self {
        Self {
            num_sectors,
            size: 0,
            marker_type,
        }
    }

    /// Serializes the marker to 512 bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![0u8; SECTOR_SIZE as usize];

        // num_sectors at offset 0 (8 bytes)
        buf[0..8].copy_from_slice(&self.num_sectors.to_le_bytes());

        // size at offset 8 (4 bytes)
        buf[8..12].copy_from_slice(&self.size.to_le_bytes());

        // marker_type at offset 12 (4 bytes)
        buf[12..16].copy_from_slice(&(self.marker_type as u32).to_le_bytes());

        buf
    }
}

/// Grain marker that precedes compressed grain data.
///
/// This is a 12-byte structure embedded before each compressed grain.
#[derive(Debug, Clone)]
pub struct GrainMarker {
    /// Logical block address of the grain (in sectors).
    pub lba: u64,
    /// Size of the compressed grain data in bytes.
    pub size: u32,
}

impl GrainMarker {
    /// Creates a new grain marker.
    pub fn new(lba: u64, size: u32) -> Self {
        Self { lba, size }
    }

    /// Serializes the grain marker to 12 bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![0u8; 12];
        buf[0..8].copy_from_slice(&self.lba.to_le_bytes());
        buf[8..12].copy_from_slice(&self.size.to_le_bytes());
        buf
    }
}

/// Sparse extent header for VMDK files.
///
/// This is a 512-byte structure at the start of the VMDK file.
#[derive(Debug, Clone)]
pub struct SparseExtentHeader {
    /// Magic number (VMDK_MAGIC).
    pub magic: u32,
    /// Version (3 for streamOptimized).
    pub version: u32,
    /// Flags.
    pub flags: u32,
    /// Capacity in sectors.
    pub capacity: u64,
    /// Grain size in sectors.
    pub grain_size: u64,
    /// Descriptor offset in sectors.
    pub descriptor_offset: u64,
    /// Descriptor size in sectors.
    pub descriptor_size: u64,
    /// Number of grain table entries per grain table.
    pub num_gtes_per_gt: u32,
    /// Redundant grain directory offset (not used for streamOptimized).
    pub rgd_offset: u64,
    /// Grain directory offset in sectors.
    pub gd_offset: u64,
    /// Overhead in sectors.
    pub overhead: u64,
    /// Unclean shutdown flag.
    pub unclean_shutdown: u8,
    /// Newline detection characters.
    pub newline_chars: [u8; 4],
    /// Compression algorithm (1 = DEFLATE).
    pub compress_algorithm: u16,
}

impl SparseExtentHeader {
    /// Creates a new sparse extent header for the given capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity_bytes` - Total disk capacity in bytes.
    pub fn new(capacity_bytes: u64) -> Self {
        let capacity_sectors = capacity_bytes / SECTOR_SIZE;

        Self {
            magic: VMDK_MAGIC,
            version: 3,
            flags: STREAM_OPTIMIZED_FLAGS,
            capacity: capacity_sectors,
            grain_size: DEFAULT_GRAIN_SIZE,
            descriptor_offset: 0,
            descriptor_size: 0,
            num_gtes_per_gt: GT_ENTRIES_PER_GT,
            rgd_offset: 0,
            gd_offset: GD_AT_END,
            overhead: 0,
            unclean_shutdown: 0,
            newline_chars: [b'\n', b' ', b'\r', b'\n'],
            compress_algorithm: COMPRESS_ALGORITHM_DEFLATE,
        }
    }

    /// Serializes the header to exactly 512 bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![0u8; SECTOR_SIZE as usize];

        // Offset 0: magic (4 bytes)
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());

        // Offset 4: version (4 bytes)
        buf[4..8].copy_from_slice(&self.version.to_le_bytes());

        // Offset 8: flags (4 bytes)
        buf[8..12].copy_from_slice(&self.flags.to_le_bytes());

        // Offset 12: capacity (8 bytes)
        buf[12..20].copy_from_slice(&self.capacity.to_le_bytes());

        // Offset 20: grainSize (8 bytes)
        buf[20..28].copy_from_slice(&self.grain_size.to_le_bytes());

        // Offset 28: descriptorOffset (8 bytes)
        buf[28..36].copy_from_slice(&self.descriptor_offset.to_le_bytes());

        // Offset 36: descriptorSize (8 bytes)
        buf[36..44].copy_from_slice(&self.descriptor_size.to_le_bytes());

        // Offset 44: numGTEsPerGT (4 bytes)
        buf[44..48].copy_from_slice(&self.num_gtes_per_gt.to_le_bytes());

        // Offset 48: rgdOffset (8 bytes)
        buf[48..56].copy_from_slice(&self.rgd_offset.to_le_bytes());

        // Offset 56: gdOffset (8 bytes)
        buf[56..64].copy_from_slice(&self.gd_offset.to_le_bytes());

        // Offset 64: overhead (8 bytes)
        buf[64..72].copy_from_slice(&self.overhead.to_le_bytes());

        // Offset 72: uncleanShutdown (1 byte)
        buf[72] = self.unclean_shutdown;

        // Offset 73: singleEndLineChar (1 byte) - '\n'
        buf[73] = self.newline_chars[0];

        // Offset 74: nonEndLineChar (1 byte) - ' '
        buf[74] = self.newline_chars[1];

        // Offset 75: doubleEndLineChar1 (1 byte) - '\r'
        buf[75] = self.newline_chars[2];

        // Offset 76: doubleEndLineChar2 (1 byte) - '\n'
        buf[76] = self.newline_chars[3];

        // Offset 77: compressAlgorithm (2 bytes)
        buf[77..79].copy_from_slice(&self.compress_algorithm.to_le_bytes());

        // Offset 79-511: pad (433 bytes, already zeroed)

        buf
    }

    /// Creates a footer header with the actual GD offset.
    pub fn as_footer(&self, gd_offset_sectors: u64) -> Self {
        let mut footer = self.clone();
        footer.gd_offset = gd_offset_sectors;
        footer
    }
}

/// Compresses grain data using DEFLATE.
///
/// # Arguments
///
/// * `data` - The uncompressed grain data.
/// * `level` - Compression level (0-9, where 6 is default).
///
/// # Returns
///
/// The compressed data as a `Vec<u8>`.
pub fn compress_grain(data: &[u8], level: u32) -> Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(level));
    encoder
        .write_all(data)
        .map_err(|e| Error::vmdk(format!("Failed to compress grain: {}", e)))?;
    encoder
        .finish()
        .map_err(|e| Error::vmdk(format!("Failed to finish compression: {}", e)))
}

/// A writer for creating streamOptimized VMDK files.
///
/// This writer creates VMware-compatible VMDK files with:
/// - StreamOptimized format (version 3)
/// - DEFLATE compression for grains
/// - Markers for metadata sections
/// - Footer with grain directory location
///
/// # Example
///
/// ```no_run
/// use ovatool_core::vmdk::stream::{StreamVmdkWriter, compress_grain};
/// use std::fs::File;
///
/// let file = File::create("output.vmdk").unwrap();
/// let mut writer = StreamVmdkWriter::new(file, 10 * 1024 * 1024 * 1024).unwrap();
///
/// // Write compressed grains
/// let grain_data = vec![0u8; 64 * 1024];
/// let compressed = compress_grain(&grain_data, 6).unwrap();
/// writer.write_grain(0, &compressed).unwrap();
///
/// // Finish writing (writes grain tables, directory, footer)
/// let _file = writer.finish().unwrap();
/// ```
pub struct StreamVmdkWriter<W: Write + Seek> {
    writer: W,
    header: SparseExtentHeader,
    /// Current position in the file (in bytes).
    current_pos: u64,
    /// Map of grain index to sector offset where grain data was written.
    grain_offsets: BTreeMap<u64, u64>,
    /// Grain size in bytes.
    grain_size_bytes: u64,
}

impl<W: Write + Seek> StreamVmdkWriter<W> {
    /// Creates a new StreamVmdkWriter.
    ///
    /// # Arguments
    ///
    /// * `writer` - The underlying writer (file, buffer, etc.).
    /// * `capacity_bytes` - Total disk capacity in bytes.
    ///
    /// # Returns
    ///
    /// A `Result` containing the writer on success.
    pub fn new(mut writer: W, capacity_bytes: u64) -> Result<Self> {
        let header = SparseExtentHeader::new(capacity_bytes);

        // Write the header
        let header_bytes = header.to_bytes();
        writer
            .write_all(&header_bytes)
            .map_err(|e| Error::vmdk(format!("Failed to write VMDK header: {}", e)))?;

        let grain_size_bytes = header.grain_size * SECTOR_SIZE;

        Ok(Self {
            writer,
            header,
            current_pos: SECTOR_SIZE,
            grain_offsets: BTreeMap::new(),
            grain_size_bytes,
        })
    }

    /// Writes a compressed grain at the specified LBA.
    ///
    /// # Arguments
    ///
    /// * `lba` - Logical block address (in sectors) of the grain.
    /// * `compressed_data` - The pre-compressed grain data.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or failure.
    pub fn write_grain(&mut self, lba: u64, compressed_data: &[u8]) -> Result<()> {
        // Calculate grain index
        let grain_index = lba / self.header.grain_size;

        // Write grain marker (12 bytes)
        let marker = GrainMarker::new(lba, compressed_data.len() as u32);
        self.writer
            .write_all(&marker.to_bytes())
            .map_err(|e| Error::vmdk(format!("Failed to write grain marker: {}", e)))?;

        // Record the offset where the grain data starts (after the marker)
        // The grain table entry points to the sector containing the grain marker
        let grain_sector = self.current_pos / SECTOR_SIZE;
        self.grain_offsets.insert(grain_index, grain_sector);

        // Write compressed data
        self.writer
            .write_all(compressed_data)
            .map_err(|e| Error::vmdk(format!("Failed to write grain data: {}", e)))?;

        // Update position
        self.current_pos += 12 + compressed_data.len() as u64;

        // Pad to sector boundary
        let remainder = self.current_pos % SECTOR_SIZE;
        if remainder != 0 {
            let padding = SECTOR_SIZE - remainder;
            let pad_bytes = vec![0u8; padding as usize];
            self.writer
                .write_all(&pad_bytes)
                .map_err(|e| Error::vmdk(format!("Failed to write padding: {}", e)))?;
            self.current_pos += padding;
        }

        Ok(())
    }

    /// Finishes writing the VMDK file.
    ///
    /// This writes the grain tables, grain directory, footer, and EOS marker.
    ///
    /// # Returns
    ///
    /// The underlying writer on success.
    pub fn finish(mut self) -> Result<W> {
        // Calculate number of grain tables needed
        let total_grains = (self.header.capacity + self.header.grain_size - 1) / self.header.grain_size;
        let num_gts = (total_grains + GT_ENTRIES_PER_GT as u64 - 1) / GT_ENTRIES_PER_GT as u64;

        // Write grain tables
        let mut gt_offsets: Vec<u64> = Vec::with_capacity(num_gts as usize);

        for gt_index in 0..num_gts {
            let gt_start_grain = gt_index * GT_ENTRIES_PER_GT as u64;

            // Build grain table entries
            let mut gt_entries = vec![0u32; GT_ENTRIES_PER_GT as usize];
            for (i, entry) in gt_entries.iter_mut().enumerate() {
                let grain_index = gt_start_grain + i as u64;
                if let Some(&offset) = self.grain_offsets.get(&grain_index) {
                    *entry = offset as u32;
                }
            }

            // Check if this GT has any entries
            let has_entries = gt_entries.iter().any(|&e| e != 0);
            if !has_entries {
                gt_offsets.push(0);
                continue;
            }

            // Write grain table marker
            let gt_size_sectors = (GT_ENTRIES_PER_GT * 4 + SECTOR_SIZE as u32 - 1) / SECTOR_SIZE as u32;
            let gt_marker = Marker::new(MarkerType::GrainTable, gt_size_sectors as u64);
            self.writer
                .write_all(&gt_marker.to_bytes())
                .map_err(|e| Error::vmdk(format!("Failed to write GT marker: {}", e)))?;

            // Record GT offset (sector after the marker)
            let gt_offset = (self.current_pos + SECTOR_SIZE) / SECTOR_SIZE;
            gt_offsets.push(gt_offset);
            self.current_pos += SECTOR_SIZE;

            // Write grain table entries
            let mut gt_bytes = Vec::with_capacity(GT_ENTRIES_PER_GT as usize * 4);
            for entry in &gt_entries {
                gt_bytes.extend_from_slice(&entry.to_le_bytes());
            }

            // Pad to sector boundary
            while gt_bytes.len() % SECTOR_SIZE as usize != 0 {
                gt_bytes.push(0);
            }

            self.writer
                .write_all(&gt_bytes)
                .map_err(|e| Error::vmdk(format!("Failed to write grain table: {}", e)))?;
            self.current_pos += gt_bytes.len() as u64;
        }

        // Write grain directory marker
        let gd_size_sectors = (num_gts * 4 + SECTOR_SIZE - 1) / SECTOR_SIZE;
        let gd_marker = Marker::new(MarkerType::GrainDirectory, gd_size_sectors);
        self.writer
            .write_all(&gd_marker.to_bytes())
            .map_err(|e| Error::vmdk(format!("Failed to write GD marker: {}", e)))?;

        // Record GD offset (sector after the marker)
        let gd_offset = (self.current_pos + SECTOR_SIZE) / SECTOR_SIZE;
        self.current_pos += SECTOR_SIZE;

        // Write grain directory entries
        let mut gd_bytes = Vec::with_capacity(num_gts as usize * 4);
        for &gt_offset in &gt_offsets {
            gd_bytes.extend_from_slice(&(gt_offset as u32).to_le_bytes());
        }

        // Pad to sector boundary
        while gd_bytes.len() % SECTOR_SIZE as usize != 0 {
            gd_bytes.push(0);
        }

        self.writer
            .write_all(&gd_bytes)
            .map_err(|e| Error::vmdk(format!("Failed to write grain directory: {}", e)))?;
        self.current_pos += gd_bytes.len() as u64;

        // Write footer marker
        let footer_marker = Marker::new(MarkerType::Footer, 1);
        self.writer
            .write_all(&footer_marker.to_bytes())
            .map_err(|e| Error::vmdk(format!("Failed to write footer marker: {}", e)))?;
        self.current_pos += SECTOR_SIZE;

        // Write footer (header with actual GD offset)
        let footer = self.header.as_footer(gd_offset);
        self.writer
            .write_all(&footer.to_bytes())
            .map_err(|e| Error::vmdk(format!("Failed to write footer: {}", e)))?;
        self.current_pos += SECTOR_SIZE;

        // Write EOS marker
        let eos_marker = Marker::new(MarkerType::EndOfStream, 0);
        self.writer
            .write_all(&eos_marker.to_bytes())
            .map_err(|e| Error::vmdk(format!("Failed to write EOS marker: {}", e)))?;

        // Flush the writer
        self.writer
            .flush()
            .map_err(|e| Error::vmdk(format!("Failed to flush VMDK: {}", e)))?;

        Ok(self.writer)
    }

    /// Returns the grain size in bytes.
    pub fn grain_size_bytes(&self) -> u64 {
        self.grain_size_bytes
    }

    /// Returns the total capacity in bytes.
    pub fn capacity_bytes(&self) -> u64 {
        self.header.capacity * SECTOR_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_vmdk_magic_value() {
        // Verify VMDK_MAGIC is correct
        let magic_bytes = VMDK_MAGIC.to_le_bytes();
        assert_eq!(&magic_bytes, b"KDMV"); // Little-endian "VMDK"
    }

    #[test]
    fn test_sparse_extent_header_new() {
        let header = SparseExtentHeader::new(1024 * 1024 * 1024);
        assert_eq!(header.magic, VMDK_MAGIC);
        assert_eq!(header.version, 3);
        assert_eq!(header.grain_size, DEFAULT_GRAIN_SIZE);
        assert_eq!(header.gd_offset, GD_AT_END);
    }

    #[test]
    fn test_marker_to_bytes() {
        let marker = Marker::new(MarkerType::GrainTable, 4);
        let bytes = marker.to_bytes();
        assert_eq!(bytes.len(), SECTOR_SIZE as usize);

        // Check num_sectors
        let num_sectors = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        assert_eq!(num_sectors, 4);

        // Check marker_type
        let marker_type =
            u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        assert_eq!(marker_type, MarkerType::GrainTable as u32);
    }

    #[test]
    fn test_grain_marker_to_bytes() {
        let marker = GrainMarker::new(128, 4096);
        let bytes = marker.to_bytes();
        assert_eq!(bytes.len(), 12);

        // Check LBA
        let lba = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        assert_eq!(lba, 128);

        // Check size
        let size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        assert_eq!(size, 4096);
    }

    #[test]
    fn test_compress_grain_basic() {
        let data = vec![0u8; 1024];
        let compressed = compress_grain(&data, 6).unwrap();
        assert!(!compressed.is_empty());
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_stream_vmdk_writer_basic() {
        let buffer = Cursor::new(Vec::new());
        let writer = StreamVmdkWriter::new(buffer, 1024 * 1024 * 1024).unwrap();
        let result = writer.finish().unwrap();
        let data = result.into_inner();

        // Should have at least header + GD + footer + EOS
        assert!(data.len() >= SECTOR_SIZE as usize * 4);

        // Check magic
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        assert_eq!(magic, VMDK_MAGIC);
    }
}
