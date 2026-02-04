//! Integration tests for OVA TAR writer with SHA256 manifest.

use ovatool_core::ova::{
    compute_sha256, create_tar_header_with_mtime, OvaWriter, Sha256Writer,
};
use std::io::{Cursor, Write};

#[test]
fn test_sha256_computation() {
    let data = b"hello world";
    let hash = compute_sha256(data);
    assert_eq!(
        hash,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn test_ova_tar_structure() {
    // Create OVA, add file, verify TAR structure (first 100 bytes contain filename)
    let buffer = Cursor::new(Vec::new());
    let mut writer = OvaWriter::new(buffer).unwrap();
    writer.add_file("test.ovf", b"<ovf content>").unwrap();
    let result = writer.finish().unwrap();

    let data = result.into_inner();

    // First 100 bytes of TAR header contain the filename
    let name_end = data[0..100].iter().position(|&b| b == 0).unwrap_or(100);
    let filename = std::str::from_utf8(&data[0..name_end]).unwrap();
    assert_eq!(filename, "test.ovf");

    // File content starts at offset 512 (after the header)
    assert_eq!(&data[512..525], b"<ovf content>");
}

#[test]
fn test_tar_header_fields() {
    let header = create_tar_header_with_mtime("myfile.vmdk", 65536, 1700000000);

    // Name at offset 0
    assert_eq!(&header[0..11], b"myfile.vmdk");
    assert_eq!(header[11], 0);

    // Mode at offset 100 (octal 0644)
    assert_eq!(&header[100..107], b"0000644");

    // UID at offset 108
    assert_eq!(&header[108..115], b"0000000");

    // GID at offset 116
    assert_eq!(&header[116..123], b"0000000");

    // Size at offset 124 (65536 = 0o200000)
    assert_eq!(&header[124..135], b"00000200000");

    // Mtime at offset 136
    assert_eq!(&header[136..147], b"14524770400"); // 1700000000 in octal

    // Type flag at offset 156 ('0' for regular file)
    assert_eq!(header[156], b'0');

    // USTAR magic at offset 257
    assert_eq!(&header[257..263], b"ustar\0");
    assert_eq!(&header[263..265], b"00");
}

#[test]
fn test_sha256_writer_incremental() {
    let buffer = Vec::new();
    let mut writer = Sha256Writer::new(buffer);

    // Write in multiple chunks
    writer.write_all(b"hello").unwrap();
    writer.write_all(b" ").unwrap();
    writer.write_all(b"world").unwrap();

    let (inner, hash, bytes) = writer.finish();

    // Should match the hash of "hello world"
    assert_eq!(
        hash,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
    assert_eq!(bytes, 11);
    assert_eq!(inner.len(), 11);
}

#[test]
fn test_ova_multiple_files() {
    let buffer = Cursor::new(Vec::new());
    let mut writer = OvaWriter::new(buffer).unwrap();

    writer.add_file("descriptor.ovf", b"OVF content").unwrap();
    writer.add_file("disk1.vmdk", b"VMDK data 1").unwrap();
    writer.add_file("disk2.vmdk", b"VMDK data 2").unwrap();

    let result = writer.finish().unwrap();
    let data = result.into_inner();

    // Find each file in the archive
    let files = extract_tar_filenames(&data);
    assert!(files.contains(&"descriptor.ovf".to_string()));
    assert!(files.contains(&"disk1.vmdk".to_string()));
    assert!(files.contains(&"disk2.vmdk".to_string()));
    assert!(files.contains(&"manifest.mf".to_string()));
}

#[test]
fn test_manifest_format() {
    let buffer = Cursor::new(Vec::new());
    let mut writer = OvaWriter::new(buffer).unwrap();

    writer.add_file("test.ovf", b"OVF content").unwrap();
    writer.add_file("test.vmdk", b"VMDK data").unwrap();

    let result = writer.finish().unwrap();
    let data = result.into_inner();

    // Extract manifest content
    let manifest = extract_file_content(&data, "manifest.mf").unwrap();
    let manifest_str = String::from_utf8_lossy(&manifest);

    // Check manifest format: SHA256(filename)= hash
    assert!(manifest_str.contains("SHA256(test.ovf)= "));
    assert!(manifest_str.contains("SHA256(test.vmdk)= "));

    // Verify the hashes are correct
    let ovf_hash = compute_sha256(b"OVF content");
    let vmdk_hash = compute_sha256(b"VMDK data");
    assert!(manifest_str.contains(&ovf_hash));
    assert!(manifest_str.contains(&vmdk_hash));
}

#[test]
fn test_streaming_file_write() {
    let buffer = Cursor::new(Vec::new());
    let mut ova = OvaWriter::new(buffer).unwrap();

    // Simulate streaming a large file
    let content = b"This is streaming content that gets written incrementally";
    {
        let mut stream = ova
            .add_file_streaming("stream.bin", content.len() as u64)
            .unwrap();

        // Write in chunks
        for chunk in content.chunks(10) {
            stream.write_all(chunk).unwrap();
        }
        stream.finish().unwrap();
    }

    let result = ova.finish().unwrap();
    let data = result.into_inner();

    // Verify the content was written correctly
    let extracted = extract_file_content(&data, "stream.bin").unwrap();
    assert_eq!(extracted, content);

    // Verify manifest contains the hash
    let manifest = extract_file_content(&data, "manifest.mf").unwrap();
    let expected_hash = compute_sha256(content);
    assert!(String::from_utf8_lossy(&manifest).contains(&expected_hash));
}

#[test]
fn test_tar_padding_to_512_boundary() {
    let buffer = Cursor::new(Vec::new());
    let mut writer = OvaWriter::new(buffer).unwrap();

    // Add a file that's not 512-byte aligned
    writer.add_file("small.txt", b"tiny").unwrap(); // 4 bytes

    let result = writer.finish().unwrap();
    let data = result.into_inner();

    // Total size must be multiple of 512
    assert_eq!(data.len() % 512, 0);

    // Structure should be:
    // 512 (header) + 512 (content padded) + 512 (manifest header) + 512 (manifest padded) + 1024 (end blocks)
    // But manifest size varies, so just check alignment
    assert!(data.len() >= 512 * 4); // At minimum: header + content + manifest header + end blocks
}

#[test]
fn test_tar_end_marker() {
    let buffer = Cursor::new(Vec::new());
    let mut writer = OvaWriter::new(buffer).unwrap();
    writer.add_file("test.txt", b"content").unwrap();
    let result = writer.finish().unwrap();
    let data = result.into_inner();

    // Last 1024 bytes should be zeros (two 512-byte end blocks)
    let end_blocks = &data[data.len() - 1024..];
    assert!(end_blocks.iter().all(|&b| b == 0));
}

#[test]
fn test_empty_file() {
    let buffer = Cursor::new(Vec::new());
    let mut writer = OvaWriter::new(buffer).unwrap();
    writer.add_file("empty.txt", b"").unwrap();

    let result = writer.finish().unwrap();
    let data = result.into_inner();

    // Should have header for empty file
    let files = extract_tar_filenames(&data);
    assert!(files.contains(&"empty.txt".to_string()));

    // Manifest should have hash of empty file
    let manifest = extract_file_content(&data, "manifest.mf").unwrap();
    let empty_hash = compute_sha256(b"");
    assert!(String::from_utf8_lossy(&manifest).contains(&empty_hash));
}

#[test]
fn test_long_filename() {
    let buffer = Cursor::new(Vec::new());
    let mut writer = OvaWriter::new(buffer).unwrap();

    // 99 character filename (max for basic TAR)
    let long_name = "a".repeat(99);
    writer.add_file(&long_name, b"content").unwrap();

    let result = writer.finish().unwrap();
    let data = result.into_inner();

    let files = extract_tar_filenames(&data);
    assert!(files.contains(&long_name));
}

// Helper functions for tests

fn extract_tar_filenames(data: &[u8]) -> Vec<String> {
    let mut filenames = Vec::new();
    let mut pos = 0;

    while pos + 512 <= data.len() {
        // Check for end of archive
        if data[pos..pos + 512].iter().all(|&b| b == 0) {
            break;
        }

        // Extract filename
        let name_end = data[pos..pos + 100]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(100);
        if let Ok(name) = std::str::from_utf8(&data[pos..pos + name_end]) {
            if !name.is_empty() {
                filenames.push(name.to_string());
            }
        }

        // Parse size and skip to next header
        if let Ok(size_str) = std::str::from_utf8(&data[pos + 124..pos + 135]) {
            if let Ok(size) = u64::from_str_radix(size_str.trim_matches('\0').trim(), 8) {
                let content_blocks = (size + 511) / 512;
                pos += 512 + (content_blocks * 512) as usize;
                continue;
            }
        }
        break;
    }

    filenames
}

fn extract_file_content(data: &[u8], filename: &str) -> Option<Vec<u8>> {
    let mut pos = 0;

    while pos + 512 <= data.len() {
        if data[pos..pos + 512].iter().all(|&b| b == 0) {
            break;
        }

        let name_end = data[pos..pos + 100]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(100);
        let name = std::str::from_utf8(&data[pos..pos + name_end]).ok()?;

        let size_str = std::str::from_utf8(&data[pos + 124..pos + 135]).ok()?;
        let size = u64::from_str_radix(size_str.trim_matches('\0').trim(), 8).ok()?;

        if name == filename {
            let content_start = pos + 512;
            let content_end = content_start + size as usize;
            return Some(data[content_start..content_end].to_vec());
        }

        let content_blocks = (size + 511) / 512;
        pos += 512 + (content_blocks * 512) as usize;
    }

    None
}
