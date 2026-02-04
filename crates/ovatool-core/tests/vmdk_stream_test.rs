//! Integration tests for StreamOptimized VMDK writer.

use ovatool_core::vmdk::stream::{
    compress_grain, SparseExtentHeader, StreamVmdkWriter, DEFAULT_GRAIN_SIZE, SECTOR_SIZE,
    VMDK_MAGIC,
};
use std::io::Cursor;

const ONE_GB: u64 = 1024 * 1024 * 1024;

#[test]
fn test_writer_magic_number() {
    // Create a writer and verify the first 4 bytes are VMDK_MAGIC (little-endian)
    let buffer = Cursor::new(Vec::new());
    let writer = StreamVmdkWriter::new(buffer, ONE_GB).expect("Failed to create writer");
    let result = writer.finish().expect("Failed to finish writer");
    let data = result.into_inner();

    // First 4 bytes should be VMDK_MAGIC in little-endian
    assert!(data.len() >= 4, "Output too small");
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    assert_eq!(
        magic, VMDK_MAGIC,
        "Magic number mismatch: expected 0x{:08X}, got 0x{:08X}",
        VMDK_MAGIC, magic
    );
}

#[test]
fn test_writer_version() {
    // Verify version is 3 (streamOptimized)
    let buffer = Cursor::new(Vec::new());
    let writer = StreamVmdkWriter::new(buffer, ONE_GB).expect("Failed to create writer");
    let result = writer.finish().expect("Failed to finish writer");
    let data = result.into_inner();

    // Version is at offset 4, 4 bytes little-endian
    assert!(data.len() >= 8, "Output too small");
    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    assert_eq!(
        version, 3,
        "Version mismatch: expected 3 (streamOptimized), got {}",
        version
    );
}

#[test]
fn test_compress_grain() {
    // Verify compress_grain compresses data using DEFLATE
    let data = vec![0u8; 64 * 1024]; // 64KB of zeros (highly compressible)
    let compressed = compress_grain(&data, 6).expect("Failed to compress grain");

    // Compressed zeros should be much smaller than original
    assert!(
        compressed.len() < data.len(),
        "Compressed size ({}) should be less than original size ({})",
        compressed.len(),
        data.len()
    );

    // Verify it's valid DEFLATE by decompressing
    use flate2::read::DeflateDecoder;
    use std::io::Read;

    let mut decoder = DeflateDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .expect("Failed to decompress");

    assert_eq!(
        decompressed, data,
        "Decompressed data should match original"
    );
}

#[test]
fn test_sparse_extent_header_size() {
    // SparseExtentHeader should serialize to exactly 512 bytes
    let header = SparseExtentHeader::new(ONE_GB);
    let bytes = header.to_bytes();

    assert_eq!(
        bytes.len(),
        SECTOR_SIZE as usize,
        "SparseExtentHeader should be exactly 512 bytes"
    );
}

#[test]
fn test_sparse_extent_header_fields() {
    // Verify key fields in the header
    let capacity_bytes = 10 * ONE_GB; // 10GB disk
    let header = SparseExtentHeader::new(capacity_bytes);
    let bytes = header.to_bytes();

    // Magic at offset 0
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    assert_eq!(magic, VMDK_MAGIC);

    // Version at offset 4
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(version, 3);

    // Capacity at offset 12 (after flags at 8)
    let capacity = u64::from_le_bytes([
        bytes[12], bytes[13], bytes[14], bytes[15], bytes[16], bytes[17], bytes[18], bytes[19],
    ]);
    let expected_capacity = capacity_bytes / SECTOR_SIZE;
    assert_eq!(
        capacity, expected_capacity,
        "Capacity should be {} sectors",
        expected_capacity
    );

    // Grain size at offset 20
    let grain_size = u64::from_le_bytes([
        bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27],
    ]);
    assert_eq!(grain_size, DEFAULT_GRAIN_SIZE);
}

#[test]
fn test_writer_writes_grain_data() {
    // Test that write_grain actually writes data
    let buffer = Cursor::new(Vec::new());
    let mut writer = StreamVmdkWriter::new(buffer, ONE_GB).expect("Failed to create writer");

    // Create and compress a grain
    let grain_data = vec![0xAB; 64 * 1024]; // 64KB grain
    let compressed = compress_grain(&grain_data, 6).expect("Failed to compress");

    // Write the grain at LBA 0
    writer.write_grain(0, &compressed).expect("Failed to write grain");

    let result = writer.finish().expect("Failed to finish");
    let data = result.into_inner();

    // The file should be larger than just the header
    assert!(
        data.len() > SECTOR_SIZE as usize,
        "Output should contain more than just the header"
    );
}

#[test]
fn test_compress_grain_random_data() {
    // Random data should still compress (though maybe not as much)
    let mut data = vec![0u8; 64 * 1024];
    // Fill with pseudo-random pattern
    for (i, byte) in data.iter_mut().enumerate() {
        *byte = ((i * 17 + 31) % 256) as u8;
    }

    let compressed = compress_grain(&data, 6).expect("Failed to compress grain");

    // Should produce valid output
    assert!(!compressed.is_empty(), "Compressed output should not be empty");

    // Verify decompression
    use flate2::read::DeflateDecoder;
    use std::io::Read;

    let mut decoder = DeflateDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .expect("Failed to decompress");

    assert_eq!(
        decompressed, data,
        "Decompressed data should match original"
    );
}
