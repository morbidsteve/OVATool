//! Sparse VMDK reader.
//!
//! This module provides functionality for reading hosted sparse VMDK files
//! (monolithicSparse, twoGbMaxExtentSparse) and extracting the virtual disk data.

use crate::error::{Error, Result};
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use super::stream::{SECTOR_SIZE, VMDK_MAGIC};

/// Flags in sparse VMDK header.
const FLAG_VALID_NEWLINE: u32 = 1 << 0;
const FLAG_REDUNDANT_GRAIN_TABLE: u32 = 1 << 1;
const FLAG_COMPRESSED: u32 = 1 << 16;
const FLAG_MARKERS: u32 = 1 << 17;

/// A reader for sparse VMDK files.
///
/// This reader handles hosted sparse VMDKs (monolithicSparse, twoGbMaxExtentSparse)
/// which store data in grain tables with optional compression.
pub struct SparseVmdkReader {
    /// Memory-mapped file data.
    mmap: Arc<Mmap>,
    /// Parsed header.
    header: SparseHeader,
    /// Grain directory entries (offsets to grain tables in sectors).
    grain_directory: Vec<u32>,
    /// Total virtual disk size in bytes.
    capacity_bytes: u64,
}

/// Parsed sparse VMDK header.
#[derive(Debug, Clone)]
struct SparseHeader {
    version: u32,
    flags: u32,
    capacity: u64,
    grain_size: u64,
    descriptor_offset: u64,
    descriptor_size: u64,
    num_gtes_per_gt: u32,
    gd_offset: u64,
}

impl SparseHeader {
    /// Parse header from bytes.
    fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 512 {
            return Err(Error::vmdk("Sparse header too short"));
        }

        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != VMDK_MAGIC {
            return Err(Error::vmdk(format!(
                "Invalid VMDK magic: expected 0x{:X}, got 0x{:X}",
                VMDK_MAGIC, magic
            )));
        }

        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let flags = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let capacity = u64::from_le_bytes([
            data[12], data[13], data[14], data[15], data[16], data[17], data[18], data[19],
        ]);
        let grain_size = u64::from_le_bytes([
            data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
        ]);
        let descriptor_offset = u64::from_le_bytes([
            data[28], data[29], data[30], data[31], data[32], data[33], data[34], data[35],
        ]);
        let descriptor_size = u64::from_le_bytes([
            data[36], data[37], data[38], data[39], data[40], data[41], data[42], data[43],
        ]);
        let num_gtes_per_gt = u32::from_le_bytes([data[44], data[45], data[46], data[47]]);
        // Skip rgdOffset at 48-55
        let gd_offset = u64::from_le_bytes([
            data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
        ]);

        Ok(Self {
            version,
            flags,
            capacity,
            grain_size,
            descriptor_offset,
            descriptor_size,
            num_gtes_per_gt,
            gd_offset,
        })
    }

    /// Check if grains are compressed.
    fn is_compressed(&self) -> bool {
        (self.flags & FLAG_COMPRESSED) != 0
    }

    /// Check if this is a streamOptimized VMDK with markers.
    fn has_markers(&self) -> bool {
        (self.flags & FLAG_MARKERS) != 0
    }

    /// Calculate the number of grain directory entries.
    fn num_gd_entries(&self) -> u64 {
        let grains_total = (self.capacity + self.grain_size - 1) / self.grain_size;
        (grains_total + self.num_gtes_per_gt as u64 - 1) / self.num_gtes_per_gt as u64
    }
}

impl SparseVmdkReader {
    /// Opens a sparse VMDK file and creates a reader.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the sparse VMDK file.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `SparseVmdkReader` on success.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| Error::io(e, path))?;
        let mmap = unsafe { Mmap::map(&file).map_err(|e| Error::io(e, path))? };

        // Parse header
        let header = SparseHeader::from_bytes(&mmap)?;

        // Validate version
        if header.version > 3 {
            return Err(Error::vmdk(format!(
                "Unsupported sparse VMDK version: {}",
                header.version
            )));
        }

        // StreamOptimized VMDKs with markers need special handling
        if header.has_markers() {
            return Err(Error::vmdk(
                "StreamOptimized VMDKs with markers are not supported for reading. \
                 Please use a flat or hosted sparse VMDK.",
            ));
        }

        // Read grain directory
        let gd_offset_bytes = header.gd_offset * SECTOR_SIZE;
        let num_gd_entries = header.num_gd_entries() as usize;

        if gd_offset_bytes as usize + num_gd_entries * 4 > mmap.len() {
            return Err(Error::vmdk("Grain directory extends beyond file"));
        }

        let mut grain_directory = Vec::with_capacity(num_gd_entries);
        for i in 0..num_gd_entries {
            let offset = gd_offset_bytes as usize + i * 4;
            let entry = u32::from_le_bytes([
                mmap[offset],
                mmap[offset + 1],
                mmap[offset + 2],
                mmap[offset + 3],
            ]);
            grain_directory.push(entry);
        }

        let capacity_bytes = header.capacity * SECTOR_SIZE;

        Ok(Self {
            mmap: Arc::new(mmap),
            header,
            grain_directory,
            capacity_bytes,
        })
    }

    /// Returns the virtual disk capacity in bytes.
    pub fn capacity(&self) -> u64 {
        self.capacity_bytes
    }

    /// Returns the grain size in bytes.
    pub fn grain_size_bytes(&self) -> u64 {
        self.header.grain_size * SECTOR_SIZE
    }

    /// Reads a grain at the given grain index.
    ///
    /// Returns the grain data, or a zero-filled buffer if the grain is not allocated.
    fn read_grain(&self, grain_index: u64) -> Result<Vec<u8>> {
        let grain_size_bytes = self.grain_size_bytes() as usize;
        let gtes_per_gt = self.header.num_gtes_per_gt as u64;

        // Find which grain table this grain belongs to
        let gt_index = grain_index / gtes_per_gt;
        let gte_index = grain_index % gtes_per_gt;

        // Get grain table offset from grain directory
        if gt_index >= self.grain_directory.len() as u64 {
            // Beyond grain directory - return zeros
            return Ok(vec![0u8; grain_size_bytes]);
        }

        let gt_offset_sectors = self.grain_directory[gt_index as usize];
        if gt_offset_sectors == 0 {
            // Grain table not allocated - return zeros
            return Ok(vec![0u8; grain_size_bytes]);
        }

        // Read grain table entry
        let gt_offset_bytes = gt_offset_sectors as u64 * SECTOR_SIZE;
        let gte_offset = gt_offset_bytes as usize + gte_index as usize * 4;

        if gte_offset + 4 > self.mmap.len() {
            return Err(Error::vmdk("Grain table entry extends beyond file"));
        }

        let grain_offset_sectors = u32::from_le_bytes([
            self.mmap[gte_offset],
            self.mmap[gte_offset + 1],
            self.mmap[gte_offset + 2],
            self.mmap[gte_offset + 3],
        ]);

        if grain_offset_sectors == 0 {
            // Grain not allocated - return zeros
            return Ok(vec![0u8; grain_size_bytes]);
        }

        // Read grain data
        let grain_offset_bytes = grain_offset_sectors as u64 * SECTOR_SIZE;

        if self.header.is_compressed() {
            // Compressed grain - need to decompress
            self.read_compressed_grain(grain_offset_bytes as usize, grain_size_bytes)
        } else {
            // Uncompressed grain - direct read
            let end = grain_offset_bytes as usize + grain_size_bytes;
            if end > self.mmap.len() {
                return Err(Error::vmdk("Grain extends beyond file"));
            }
            Ok(self.mmap[grain_offset_bytes as usize..end].to_vec())
        }
    }

    /// Reads and decompresses a compressed grain.
    fn read_compressed_grain(&self, offset: usize, uncompressed_size: usize) -> Result<Vec<u8>> {
        // Compressed grains have a 12-byte header: LBA (8 bytes) + size (4 bytes)
        if offset + 12 > self.mmap.len() {
            return Err(Error::vmdk("Compressed grain header extends beyond file"));
        }

        let compressed_size = u32::from_le_bytes([
            self.mmap[offset + 8],
            self.mmap[offset + 9],
            self.mmap[offset + 10],
            self.mmap[offset + 11],
        ]) as usize;

        let data_offset = offset + 12;
        if data_offset + compressed_size > self.mmap.len() {
            return Err(Error::vmdk("Compressed grain data extends beyond file"));
        }

        let compressed_data = &self.mmap[data_offset..data_offset + compressed_size];

        // Decompress using DEFLATE
        use flate2::read::DeflateDecoder;
        use std::io::Read;

        let mut decoder = DeflateDecoder::new(compressed_data);
        let mut decompressed = vec![0u8; uncompressed_size];
        decoder
            .read_exact(&mut decompressed)
            .map_err(|e| Error::vmdk(format!("Failed to decompress grain: {}", e)))?;

        Ok(decompressed)
    }

    /// Creates an iterator that yields chunks of the virtual disk.
    ///
    /// # Arguments
    ///
    /// * `chunk_size` - The size of each chunk in bytes.
    pub fn chunks(&self, chunk_size: usize) -> SparseChunkIterator {
        SparseChunkIterator::new(self, chunk_size)
    }
}

/// Iterator over chunks of a sparse VMDK.
pub struct SparseChunkIterator<'a> {
    reader: &'a SparseVmdkReader,
    chunk_size: usize,
    current_offset: u64,
}

impl<'a> SparseChunkIterator<'a> {
    fn new(reader: &'a SparseVmdkReader, chunk_size: usize) -> Self {
        Self {
            reader,
            chunk_size,
            current_offset: 0,
        }
    }

    /// Returns the total number of chunks.
    pub fn count_chunks(&self) -> usize {
        if self.reader.capacity_bytes == 0 {
            return 0;
        }
        let full_chunks = self.reader.capacity_bytes / self.chunk_size as u64;
        let remainder = self.reader.capacity_bytes % self.chunk_size as u64;
        (full_chunks + if remainder > 0 { 1 } else { 0 }) as usize
    }
}

impl<'a> Iterator for SparseChunkIterator<'a> {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_offset >= self.reader.capacity_bytes {
            return None;
        }

        let remaining = self.reader.capacity_bytes - self.current_offset;
        let chunk_len = std::cmp::min(remaining, self.chunk_size as u64) as usize;

        // Build chunk by reading grains
        let mut chunk_data = Vec::with_capacity(chunk_len);
        let grain_size_bytes = self.reader.grain_size_bytes();

        let mut bytes_read = 0u64;
        while bytes_read < chunk_len as u64 {
            let current_pos = self.current_offset + bytes_read;
            let grain_index = current_pos / grain_size_bytes;
            let offset_in_grain = (current_pos % grain_size_bytes) as usize;

            // Read grain
            let grain_data = match self.reader.read_grain(grain_index) {
                Ok(data) => data,
                Err(e) => return Some(Err(e)),
            };

            // Calculate how much to take from this grain
            let bytes_needed = chunk_len as u64 - bytes_read;
            let bytes_available = grain_size_bytes - offset_in_grain as u64;
            let bytes_to_take = std::cmp::min(bytes_needed, bytes_available) as usize;

            chunk_data.extend_from_slice(&grain_data[offset_in_grain..offset_in_grain + bytes_to_take]);
            bytes_read += bytes_to_take as u64;
        }

        self.current_offset += chunk_len as u64;
        Some(Ok(chunk_data))
    }
}

/// Check if a file is a sparse VMDK by reading its magic number.
pub fn is_sparse_vmdk(path: &Path) -> Result<bool> {
    use std::io::Read;

    let mut file = File::open(path).map_err(|e| Error::io(e, path))?;
    let mut magic_bytes = [0u8; 4];

    match file.read_exact(&mut magic_bytes) {
        Ok(_) => {
            let magic = u32::from_le_bytes(magic_bytes);
            Ok(magic == VMDK_MAGIC)
        }
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_header_from_bytes() {
        let mut header_bytes = vec![0u8; 512];
        // Magic
        header_bytes[0..4].copy_from_slice(&VMDK_MAGIC.to_le_bytes());
        // Version
        header_bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        // Flags
        header_bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        // Capacity (1000 sectors)
        header_bytes[12..20].copy_from_slice(&1000u64.to_le_bytes());
        // Grain size (128 sectors)
        header_bytes[20..28].copy_from_slice(&128u64.to_le_bytes());
        // Descriptor offset (1 sector)
        header_bytes[28..36].copy_from_slice(&1u64.to_le_bytes());
        // Descriptor size (20 sectors)
        header_bytes[36..44].copy_from_slice(&20u64.to_le_bytes());
        // numGTEsPerGT (512)
        header_bytes[44..48].copy_from_slice(&512u32.to_le_bytes());
        // rgdOffset (0)
        header_bytes[48..56].copy_from_slice(&0u64.to_le_bytes());
        // gdOffset (100)
        header_bytes[56..64].copy_from_slice(&100u64.to_le_bytes());

        let header = SparseHeader::from_bytes(&header_bytes).unwrap();
        assert_eq!(header.version, 1);
        assert_eq!(header.capacity, 1000);
        assert_eq!(header.grain_size, 128);
        assert_eq!(header.gd_offset, 100);
    }

    #[test]
    fn test_invalid_magic() {
        let header_bytes = vec![0u8; 512];
        let result = SparseHeader::from_bytes(&header_bytes);
        assert!(result.is_err());
    }
}
