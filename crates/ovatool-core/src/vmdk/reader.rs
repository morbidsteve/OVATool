//! VMDK file reader with memory-mapped I/O.
//!
//! This module provides efficient reading of VMDK files using memory mapping,
//! with support for chunked iteration suitable for parallel processing.

use crate::error::{Error, Result};
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

/// A memory-mapped VMDK file reader.
///
/// This reader uses memory mapping for efficient access to VMDK file contents,
/// allowing the operating system to manage caching and paging automatically.
///
/// # Example
///
/// ```no_run
/// use ovatool_core::vmdk::reader::VmdkReader;
/// use std::path::Path;
///
/// let reader = VmdkReader::open(Path::new("disk.vmdk")).unwrap();
/// println!("File size: {} bytes", reader.size());
///
/// // Iterate over 256KB chunks
/// for chunk_result in reader.chunks(256 * 1024) {
///     let chunk = chunk_result.unwrap();
///     // Process chunk...
/// }
/// ```
pub struct VmdkReader {
    /// The memory-mapped file data.
    mmap: Arc<Mmap>,
    /// The size of the file in bytes.
    size: u64,
}

impl VmdkReader {
    /// Opens a VMDK file and creates a memory-mapped reader.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the VMDK file to open.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `VmdkReader` on success, or an error if the
    /// file cannot be opened or memory-mapped.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file does not exist
    /// - The file cannot be opened (permissions, etc.)
    /// - Memory mapping fails
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| Error::io(e, path))?;

        let metadata = file.metadata().map_err(|e| Error::io(e, path))?;
        let size = metadata.len();

        // Handle empty files - mmap doesn't work with empty files
        if size == 0 {
            // For empty files, create a reader with empty data
            // We'll handle this specially in the iterator
            return Ok(Self {
                mmap: Arc::new(unsafe { Mmap::map(&file).map_err(|e| Error::io(e, path))? }),
                size: 0,
            });
        }

        // Safety: We're mapping a read-only file that we just opened.
        // The file will remain valid for the lifetime of the Mmap.
        let mmap = unsafe { Mmap::map(&file).map_err(|e| Error::io(e, path))? };

        Ok(Self {
            mmap: Arc::new(mmap),
            size,
        })
    }

    /// Returns the size of the VMDK file in bytes.
    #[inline]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns a reference to the raw memory-mapped data.
    ///
    /// This provides direct access to the file contents for cases where
    /// chunk iteration is not needed.
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.mmap
    }

    /// Creates an iterator that yields chunks of the file data.
    ///
    /// # Arguments
    ///
    /// * `chunk_size` - The size of each chunk in bytes. The last chunk
    ///   may be smaller if the file size is not evenly divisible.
    ///
    /// # Returns
    ///
    /// A `ChunkIterator` that yields `Result<Vec<u8>>` for each chunk.
    pub fn chunks(&self, chunk_size: usize) -> ChunkIterator {
        ChunkIterator::new(Arc::clone(&self.mmap), self.size, chunk_size)
    }

    /// Creates an iterator that yields indexed chunks of the file data.
    ///
    /// Similar to `chunks()`, but each item includes the chunk index and
    /// a flag indicating whether it's the last chunk.
    ///
    /// # Arguments
    ///
    /// * `chunk_size` - The size of each chunk in bytes.
    ///
    /// # Returns
    ///
    /// An `IndexedChunkIterator` that yields `Result<IndexedChunk>` for each chunk.
    pub fn indexed_chunks(&self, chunk_size: usize) -> IndexedChunkIterator {
        IndexedChunkIterator::new(Arc::clone(&self.mmap), self.size, chunk_size)
    }
}

/// An iterator over chunks of a memory-mapped file.
///
/// Each iteration yields a `Result<Vec<u8>>` containing the chunk data.
/// The last chunk may be smaller than `chunk_size` if the file size is
/// not evenly divisible by the chunk size.
pub struct ChunkIterator {
    mmap: Arc<Mmap>,
    file_size: u64,
    chunk_size: usize,
    current_offset: u64,
}

impl ChunkIterator {
    fn new(mmap: Arc<Mmap>, file_size: u64, chunk_size: usize) -> Self {
        Self {
            mmap,
            file_size,
            chunk_size,
            current_offset: 0,
        }
    }

    /// Returns the total number of chunks that will be yielded.
    ///
    /// This is calculated based on the file size and chunk size, accounting
    /// for a potentially smaller final chunk.
    pub fn count_chunks(&self) -> usize {
        if self.file_size == 0 {
            return 0;
        }
        let full_chunks = self.file_size / self.chunk_size as u64;
        let remainder = self.file_size % self.chunk_size as u64;
        (full_chunks + if remainder > 0 { 1 } else { 0 }) as usize
    }
}

impl Iterator for ChunkIterator {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_offset >= self.file_size {
            return None;
        }

        let remaining = self.file_size - self.current_offset;
        let chunk_len = std::cmp::min(remaining, self.chunk_size as u64) as usize;

        let start = self.current_offset as usize;
        let end = start + chunk_len;

        // Copy the chunk data
        let chunk_data = self.mmap[start..end].to_vec();

        self.current_offset += chunk_len as u64;

        Some(Ok(chunk_data))
    }
}

/// A chunk with its index and metadata.
///
/// This struct is returned by `IndexedChunkIterator` and includes:
/// - The chunk's sequential index (0-based)
/// - The chunk data
/// - A flag indicating if this is the last chunk
#[derive(Debug, Clone)]
pub struct IndexedChunk {
    /// The zero-based index of this chunk in the sequence.
    pub index: u64,
    /// The chunk data.
    pub data: Vec<u8>,
    /// True if this is the last chunk in the file.
    pub is_last: bool,
}

/// An iterator over indexed chunks of a memory-mapped file.
///
/// Similar to `ChunkIterator`, but each item includes the chunk index
/// and a flag indicating whether it's the last chunk. This is useful
/// for parallel processing scenarios where chunk ordering matters.
pub struct IndexedChunkIterator {
    mmap: Arc<Mmap>,
    file_size: u64,
    chunk_size: usize,
    current_offset: u64,
    current_index: u64,
    total_chunks: u64,
}

impl IndexedChunkIterator {
    fn new(mmap: Arc<Mmap>, file_size: u64, chunk_size: usize) -> Self {
        let total_chunks = if file_size == 0 {
            0
        } else {
            let full_chunks = file_size / chunk_size as u64;
            let remainder = file_size % chunk_size as u64;
            full_chunks + if remainder > 0 { 1 } else { 0 }
        };

        Self {
            mmap,
            file_size,
            chunk_size,
            current_offset: 0,
            current_index: 0,
            total_chunks,
        }
    }

    /// Returns the total number of chunks that will be yielded.
    pub fn count_chunks(&self) -> usize {
        self.total_chunks as usize
    }
}

impl Iterator for IndexedChunkIterator {
    type Item = Result<IndexedChunk>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_offset >= self.file_size {
            return None;
        }

        let remaining = self.file_size - self.current_offset;
        let chunk_len = std::cmp::min(remaining, self.chunk_size as u64) as usize;

        let start = self.current_offset as usize;
        let end = start + chunk_len;

        // Copy the chunk data
        let chunk_data = self.mmap[start..end].to_vec();

        let index = self.current_index;
        let is_last = self.current_index == self.total_chunks - 1;

        self.current_offset += chunk_len as u64;
        self.current_index += 1;

        Some(Ok(IndexedChunk {
            index,
            data: chunk_data,
            is_last,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_file(size: usize) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        let pattern: Vec<u8> = (0u8..=255).cycle().take(size).collect();
        file.write_all(&pattern).expect("Failed to write test data");
        file.flush().expect("Failed to flush");
        file
    }

    #[test]
    fn test_open_and_size() {
        let file = create_test_file(1024);
        let reader = VmdkReader::open(file.path()).unwrap();
        assert_eq!(reader.size(), 1024);
    }

    #[test]
    fn test_data_access() {
        let file = create_test_file(256);
        let reader = VmdkReader::open(file.path()).unwrap();
        let data = reader.data();
        assert_eq!(data.len(), 256);
        // Verify pattern
        for (i, &byte) in data.iter().enumerate() {
            assert_eq!(byte, i as u8);
        }
    }

    #[test]
    fn test_chunk_iterator_basic() {
        let file = create_test_file(1000);
        let reader = VmdkReader::open(file.path()).unwrap();
        let chunks: Vec<_> = reader.chunks(256).collect();
        assert_eq!(chunks.len(), 4); // 256 + 256 + 256 + 232 = 1000
    }

    #[test]
    fn test_indexed_chunk_is_last() {
        let file = create_test_file(512);
        let reader = VmdkReader::open(file.path()).unwrap();
        let chunks: Vec<_> = reader
            .indexed_chunks(256)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(chunks.len(), 2);
        assert!(!chunks[0].is_last);
        assert!(chunks[1].is_last);
    }
}
