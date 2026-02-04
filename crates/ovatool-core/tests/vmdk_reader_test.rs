//! Integration tests for VMDK reader with memory-mapped chunks.

use ovatool_core::vmdk::reader::{IndexedChunk, VmdkReader};
use std::io::Write;
use tempfile::NamedTempFile;

const ONE_MB: usize = 1024 * 1024;
const CHUNK_256KB: usize = 256 * 1024;

/// Helper to create a temp file with specified size filled with a pattern.
fn create_test_file(size: usize) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    // Fill with a repeating pattern for verification
    let pattern: Vec<u8> = (0u8..=255).cycle().take(size).collect();
    file.write_all(&pattern).expect("Failed to write test data");
    file.flush().expect("Failed to flush");
    file
}

#[test]
fn test_reader_chunk_iteration() {
    // Create 1MB test file, iterate with 256KB chunks, expect 4 chunks
    let file = create_test_file(ONE_MB);

    let reader = VmdkReader::open(file.path()).expect("Failed to open file");
    let chunks: Vec<Vec<u8>> = reader
        .chunks(CHUNK_256KB)
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to iterate chunks");

    assert_eq!(
        chunks.len(),
        4,
        "Expected 4 chunks for 1MB file with 256KB chunks"
    );

    // Each chunk should be exactly 256KB
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.len(), CHUNK_256KB, "Chunk {} should be 256KB", i);
    }
}

#[test]
fn test_reader_last_chunk_size() {
    // Create 1MB+100 bytes file, 256KB chunks, last chunk should be 100 bytes
    let file = create_test_file(ONE_MB + 100);

    let reader = VmdkReader::open(file.path()).expect("Failed to open file");
    let chunks: Vec<Vec<u8>> = reader
        .chunks(CHUNK_256KB)
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to iterate chunks");

    assert_eq!(chunks.len(), 5, "Expected 5 chunks for 1MB+100 bytes file");

    // First 4 chunks should be 256KB
    for i in 0..4 {
        assert_eq!(chunks[i].len(), CHUNK_256KB, "Chunk {} should be 256KB", i);
    }

    // Last chunk should be 100 bytes
    assert_eq!(chunks[4].len(), 100, "Last chunk should be 100 bytes");
}

#[test]
fn test_reader_file_size() {
    // Verify size() returns correct file size
    let file = create_test_file(ONE_MB);

    let reader = VmdkReader::open(file.path()).expect("Failed to open file");
    assert_eq!(reader.size(), ONE_MB as u64, "Size should match file size");
}

#[test]
fn test_reader_data_access() {
    // Create a small file and verify raw data access
    let size = 1024;
    let file = create_test_file(size);

    let reader = VmdkReader::open(file.path()).expect("Failed to open file");
    let data = reader.data();

    assert_eq!(data.len(), size, "Data length should match file size");

    // Verify the pattern
    for (i, &byte) in data.iter().enumerate() {
        assert_eq!(
            byte,
            (i % 256) as u8,
            "Data pattern mismatch at position {}",
            i
        );
    }
}

#[test]
fn test_chunk_iterator_count() {
    let file = create_test_file(ONE_MB);

    let reader = VmdkReader::open(file.path()).expect("Failed to open file");
    let iterator = reader.chunks(CHUNK_256KB);

    assert_eq!(
        iterator.count_chunks(),
        4,
        "count_chunks should return 4 for 1MB file with 256KB chunks"
    );
}

#[test]
fn test_chunk_iterator_count_with_remainder() {
    let file = create_test_file(ONE_MB + 100);

    let reader = VmdkReader::open(file.path()).expect("Failed to open file");
    let iterator = reader.chunks(CHUNK_256KB);

    assert_eq!(
        iterator.count_chunks(),
        5,
        "count_chunks should return 5 for 1MB+100 bytes file with 256KB chunks"
    );
}

#[test]
fn test_indexed_chunk_iteration() {
    let file = create_test_file(ONE_MB);

    let reader = VmdkReader::open(file.path()).expect("Failed to open file");
    let indexed_chunks: Vec<IndexedChunk> = reader
        .indexed_chunks(CHUNK_256KB)
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to iterate indexed chunks");

    assert_eq!(indexed_chunks.len(), 4, "Expected 4 indexed chunks");

    for (i, chunk) in indexed_chunks.iter().enumerate() {
        assert_eq!(
            chunk.index, i as u64,
            "Chunk index should match iteration order"
        );
        assert_eq!(chunk.data.len(), CHUNK_256KB, "Chunk {} should be 256KB", i);

        // Only the last chunk should have is_last = true
        if i == 3 {
            assert!(chunk.is_last, "Last chunk should have is_last = true");
        } else {
            assert!(!chunk.is_last, "Chunk {} should have is_last = false", i);
        }
    }
}

#[test]
fn test_indexed_chunk_last_partial() {
    let file = create_test_file(ONE_MB + 100);

    let reader = VmdkReader::open(file.path()).expect("Failed to open file");
    let indexed_chunks: Vec<IndexedChunk> = reader
        .indexed_chunks(CHUNK_256KB)
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to iterate indexed chunks");

    assert_eq!(indexed_chunks.len(), 5, "Expected 5 indexed chunks");

    let last = &indexed_chunks[4];
    assert_eq!(last.index, 4, "Last chunk index should be 4");
    assert_eq!(last.data.len(), 100, "Last chunk should be 100 bytes");
    assert!(last.is_last, "Last chunk should have is_last = true");
}

#[test]
fn test_empty_file() {
    let file = NamedTempFile::new().expect("Failed to create temp file");
    // Don't write anything - empty file

    let reader = VmdkReader::open(file.path()).expect("Failed to open empty file");
    assert_eq!(reader.size(), 0, "Empty file should have size 0");

    let chunks: Vec<Vec<u8>> = reader
        .chunks(CHUNK_256KB)
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to iterate chunks");

    assert_eq!(chunks.len(), 0, "Empty file should yield no chunks");
}

#[test]
fn test_chunk_data_integrity() {
    // Verify that chunk data matches the original file content
    let file = create_test_file(ONE_MB);

    let reader = VmdkReader::open(file.path()).expect("Failed to open file");
    let chunks: Vec<Vec<u8>> = reader
        .chunks(CHUNK_256KB)
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to iterate chunks");

    // Reconstruct the file from chunks
    let reconstructed: Vec<u8> = chunks.into_iter().flatten().collect();

    // Compare with direct data access
    let original = reader.data();
    assert_eq!(
        reconstructed.len(),
        original.len(),
        "Reconstructed size should match original"
    );
    assert_eq!(
        reconstructed, original,
        "Reconstructed data should match original"
    );
}

#[test]
fn test_nonexistent_file() {
    let result = VmdkReader::open(std::path::Path::new("/nonexistent/path/file.vmdk"));
    assert!(result.is_err(), "Opening nonexistent file should fail");
}
