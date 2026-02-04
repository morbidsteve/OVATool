//! OVA archive creation.
//!
//! This module handles creating OVA (Open Virtual Appliance) archives,
//! which are TAR files containing OVF descriptors and disk images.
//!
//! # OVA Format
//!
//! An OVA file is a TAR archive containing:
//! 1. An OVF descriptor file (XML)
//! 2. One or more VMDK disk images
//! 3. Optionally, a manifest file (.mf) with SHA256 checksums
//!
//! # Example
//!
//! ```no_run
//! use ovatool_core::ova::OvaWriter;
//! use std::fs::File;
//!
//! let file = File::create("output.ova").unwrap();
//! let mut writer = OvaWriter::new(file).unwrap();
//! writer.add_file("descriptor.ovf", b"<OVF content>").unwrap();
//! writer.add_file("disk.vmdk", b"<VMDK content>").unwrap();
//! writer.finish().unwrap();
//! ```

use sha2::{Digest, Sha256};
use std::io::{self, Seek, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

/// Compute SHA256 hash of data and return as hex string.
pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode(&result)
}

/// Encode bytes as lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// A writer wrapper that computes SHA256 hash while writing.
///
/// This allows computing the hash of data as it streams through,
/// avoiding the need to buffer the entire content in memory.
pub struct Sha256Writer<W: Write> {
    inner: W,
    hasher: Sha256,
    bytes_written: u64,
}

impl<W: Write> Sha256Writer<W> {
    /// Create a new SHA256 writer wrapping the given writer.
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes_written: 0,
        }
    }

    /// Finish writing and return the inner writer, hex hash, and bytes written.
    pub fn finish(self) -> (W, String, u64) {
        let hash = hex_encode(&self.hasher.finalize());
        (self.inner, hash, self.bytes_written)
    }
}

impl<W: Write> Write for Sha256Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes_written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Create a USTAR TAR header for a regular file.
///
/// # Arguments
///
/// * `name` - The filename (max 100 bytes including null terminator)
/// * `size` - The file size in bytes
///
/// # Returns
///
/// A 512-byte TAR header block.
pub fn create_tar_header(name: &str, size: u64) -> [u8; 512] {
    let mut header = [0u8; 512];

    // Name at offset 0 (100 bytes, null-terminated)
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(99);
    header[..name_len].copy_from_slice(&name_bytes[..name_len]);

    // Mode at offset 100 (8 bytes, octal "0000644\0")
    header[100..107].copy_from_slice(b"0000644");
    header[107] = 0;

    // UID at offset 108 (8 bytes, octal "0000000\0")
    header[108..115].copy_from_slice(b"0000000");
    header[115] = 0;

    // GID at offset 116 (8 bytes, octal "0000000\0")
    header[116..123].copy_from_slice(b"0000000");
    header[123] = 0;

    // Size at offset 124 (12 bytes, octal with null/space terminator)
    let size_str = format!("{:011o}", size);
    header[124..135].copy_from_slice(size_str.as_bytes());
    header[135] = 0;

    // Mtime at offset 136 (12 bytes, octal unix timestamp)
    let mtime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mtime_str = format!("{:011o}", mtime);
    header[136..147].copy_from_slice(mtime_str.as_bytes());
    header[147] = 0;

    // Checksum placeholder at offset 148 (8 bytes of spaces for initial calculation)
    header[148..156].copy_from_slice(b"        ");

    // Type flag at offset 156 (1 byte, '0' for regular file)
    header[156] = b'0';

    // Link name at offset 157 (100 bytes, empty for regular files)
    // Already zeros

    // USTAR indicator at offset 257 ("ustar\0" + version "00")
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");

    // User name at offset 265 (32 bytes)
    header[265..269].copy_from_slice(b"root");

    // Group name at offset 297 (32 bytes)
    header[297..301].copy_from_slice(b"root");

    // Calculate checksum (sum of all bytes, treating checksum field as spaces)
    let checksum: u32 = header.iter().map(|&b| b as u32).sum();
    let checksum_str = format!("{:06o}\0 ", checksum);
    header[148..156].copy_from_slice(checksum_str.as_bytes());

    header
}

/// Create a USTAR TAR header with a specific timestamp (for testing).
pub fn create_tar_header_with_mtime(name: &str, size: u64, mtime: u64) -> [u8; 512] {
    let mut header = [0u8; 512];

    // Name at offset 0 (100 bytes, null-terminated)
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(99);
    header[..name_len].copy_from_slice(&name_bytes[..name_len]);

    // Mode at offset 100 (8 bytes, octal "0000644\0")
    header[100..107].copy_from_slice(b"0000644");
    header[107] = 0;

    // UID at offset 108 (8 bytes, octal "0000000\0")
    header[108..115].copy_from_slice(b"0000000");
    header[115] = 0;

    // GID at offset 116 (8 bytes, octal "0000000\0")
    header[116..123].copy_from_slice(b"0000000");
    header[123] = 0;

    // Size at offset 124 (12 bytes, octal with null/space terminator)
    let size_str = format!("{:011o}", size);
    header[124..135].copy_from_slice(size_str.as_bytes());
    header[135] = 0;

    // Mtime at offset 136 (12 bytes, octal unix timestamp)
    let mtime_str = format!("{:011o}", mtime);
    header[136..147].copy_from_slice(mtime_str.as_bytes());
    header[147] = 0;

    // Checksum placeholder at offset 148 (8 bytes of spaces for initial calculation)
    header[148..156].copy_from_slice(b"        ");

    // Type flag at offset 156 (1 byte, '0' for regular file)
    header[156] = b'0';

    // Link name at offset 157 (100 bytes, empty for regular files)
    // Already zeros

    // USTAR indicator at offset 257 ("ustar\0" + version "00")
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");

    // User name at offset 265 (32 bytes)
    header[265..269].copy_from_slice(b"root");

    // Group name at offset 297 (32 bytes)
    header[297..301].copy_from_slice(b"root");

    // Calculate checksum (sum of all bytes, treating checksum field as spaces)
    let checksum: u32 = header.iter().map(|&b| b as u32).sum();
    let checksum_str = format!("{:06o}\0 ", checksum);
    header[148..156].copy_from_slice(checksum_str.as_bytes());

    header
}

/// Entry tracking file hash for manifest generation.
struct ManifestEntry {
    filename: String,
    hash: String,
}

/// OVA archive writer that creates TAR files with SHA256 manifest.
///
/// Files are written to the TAR archive as they are added. When `finish()`
/// is called, the manifest file is generated and appended, followed by
/// the TAR end-of-archive marker (two 512-byte zero blocks).
pub struct OvaWriter<W: Write + Seek> {
    writer: W,
    entries: Vec<ManifestEntry>,
    current_position: u64,
}

impl<W: Write + Seek> OvaWriter<W> {
    /// Create a new OVA writer.
    ///
    /// # Arguments
    ///
    /// * `writer` - The underlying writer (typically a file)
    ///
    /// # Returns
    ///
    /// A new OvaWriter ready to accept files.
    pub fn new(writer: W) -> Result<Self> {
        Ok(Self {
            writer,
            entries: Vec::new(),
            current_position: 0,
        })
    }

    /// Add a file to the OVA archive.
    ///
    /// The file is immediately written to the archive and its hash
    /// is recorded for the manifest.
    ///
    /// # Arguments
    ///
    /// * `name` - The filename within the archive
    /// * `data` - The file contents
    pub fn add_file(&mut self, name: &str, data: &[u8]) -> Result<()> {
        // Compute hash
        let hash = compute_sha256(data);

        // Write TAR header
        let header = create_tar_header(name, data.len() as u64);
        self.writer
            .write_all(&header)
            .map_err(|e| Error::ova(format!("failed to write TAR header: {}", e)))?;
        self.current_position += 512;

        // Write file data
        self.writer
            .write_all(data)
            .map_err(|e| Error::ova(format!("failed to write file data: {}", e)))?;
        self.current_position += data.len() as u64;

        // Pad to 512-byte boundary
        let padding_needed = (512 - (data.len() % 512)) % 512;
        if padding_needed > 0 {
            let padding = vec![0u8; padding_needed];
            self.writer
                .write_all(&padding)
                .map_err(|e| Error::ova(format!("failed to write padding: {}", e)))?;
            self.current_position += padding_needed as u64;
        }

        // Record for manifest
        self.entries.push(ManifestEntry {
            filename: name.to_string(),
            hash,
        });

        Ok(())
    }

    /// Begin adding a large file to the OVA archive using streaming.
    ///
    /// This is useful for large files that shouldn't be buffered entirely
    /// in memory. The caller writes data to the returned `StreamingFileWriter`,
    /// which computes the hash incrementally.
    ///
    /// # Arguments
    ///
    /// * `name` - The filename within the archive
    /// * `size` - The exact size of the file in bytes (must be known in advance)
    ///
    /// # Returns
    ///
    /// A `StreamingFileWriter` that the caller writes to.
    pub fn add_file_streaming(&mut self, name: &str, size: u64) -> Result<StreamingFileWriter<'_, W>> {
        // Write TAR header
        let header = create_tar_header(name, size);
        self.writer
            .write_all(&header)
            .map_err(|e| Error::ova(format!("failed to write TAR header: {}", e)))?;
        self.current_position += 512;

        Ok(StreamingFileWriter {
            ova_writer: self,
            filename: name.to_string(),
            expected_size: size,
            hasher: Sha256::new(),
            bytes_written: 0,
        })
    }

    /// Finish writing the OVA archive.
    ///
    /// This writes the manifest file (if any files were added) and the
    /// TAR end-of-archive marker (two 512-byte zero blocks).
    ///
    /// # Returns
    ///
    /// The underlying writer.
    pub fn finish(mut self) -> Result<W> {
        // Generate and write manifest if we have entries
        if !self.entries.is_empty() {
            let manifest = self.generate_manifest();
            let manifest_bytes = manifest.as_bytes();

            // Write manifest file
            let header = create_tar_header("manifest.mf", manifest_bytes.len() as u64);
            self.writer
                .write_all(&header)
                .map_err(|e| Error::ova(format!("failed to write manifest header: {}", e)))?;

            self.writer
                .write_all(manifest_bytes)
                .map_err(|e| Error::ova(format!("failed to write manifest: {}", e)))?;

            // Pad manifest to 512-byte boundary
            let padding_needed = (512 - (manifest_bytes.len() % 512)) % 512;
            if padding_needed > 0 {
                let padding = vec![0u8; padding_needed];
                self.writer
                    .write_all(&padding)
                    .map_err(|e| Error::ova(format!("failed to write manifest padding: {}", e)))?;
            }
        }

        // Write TAR end-of-archive marker (two 512-byte zero blocks)
        let end_marker = [0u8; 1024];
        self.writer
            .write_all(&end_marker)
            .map_err(|e| Error::ova(format!("failed to write TAR end marker: {}", e)))?;

        Ok(self.writer)
    }

    /// Generate manifest content.
    fn generate_manifest(&self) -> String {
        self.entries
            .iter()
            .map(|entry| format!("SHA256({})= {}\n", entry.filename, entry.hash))
            .collect()
    }
}

/// A writer for streaming large files into an OVA archive.
///
/// This struct wraps the OVA writer and computes the SHA256 hash
/// incrementally as data is written. When finished, it pads the
/// file to a 512-byte boundary and records the hash for the manifest.
pub struct StreamingFileWriter<'a, W: Write + Seek> {
    ova_writer: &'a mut OvaWriter<W>,
    filename: String,
    expected_size: u64,
    hasher: Sha256,
    bytes_written: u64,
}

impl<'a, W: Write + Seek> StreamingFileWriter<'a, W> {
    /// Finish writing the file.
    ///
    /// This pads the file to a 512-byte boundary and records
    /// the hash for the manifest.
    ///
    /// # Returns
    ///
    /// Error if the wrong number of bytes were written.
    pub fn finish(self) -> Result<()> {
        if self.bytes_written != self.expected_size {
            return Err(Error::ova(format!(
                "expected {} bytes but wrote {} bytes for file '{}'",
                self.expected_size, self.bytes_written, self.filename
            )));
        }

        // Compute final hash
        let hash = hex_encode(&self.hasher.finalize());

        // Update position
        self.ova_writer.current_position += self.bytes_written;

        // Pad to 512-byte boundary
        let padding_needed = (512 - (self.bytes_written as usize % 512)) % 512;
        if padding_needed > 0 {
            let padding = vec![0u8; padding_needed];
            self.ova_writer
                .writer
                .write_all(&padding)
                .map_err(|e| Error::ova(format!("failed to write padding: {}", e)))?;
            self.ova_writer.current_position += padding_needed as u64;
        }

        // Record for manifest
        self.ova_writer.entries.push(ManifestEntry {
            filename: self.filename,
            hash,
        });

        Ok(())
    }
}

impl<'a, W: Write + Seek> Write for StreamingFileWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Check if this would exceed expected size
        if self.bytes_written + buf.len() as u64 > self.expected_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "write would exceed expected size of {} bytes",
                    self.expected_size
                ),
            ));
        }

        let n = self.ova_writer.writer.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes_written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.ova_writer.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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
    fn test_sha256_empty() {
        let data = b"";
        let hash = compute_sha256(data);
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_writer() {
        let buffer = Vec::new();
        let mut writer = Sha256Writer::new(buffer);
        writer.write_all(b"hello ").unwrap();
        writer.write_all(b"world").unwrap();
        let (_, hash, bytes) = writer.finish();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(bytes, 11);
    }

    #[test]
    fn test_tar_header_name() {
        let header = create_tar_header("test.ovf", 100);
        // Name starts at offset 0
        assert_eq!(&header[0..8], b"test.ovf");
        assert_eq!(header[8], 0); // null terminated
    }

    #[test]
    fn test_tar_header_mode() {
        let header = create_tar_header("test.ovf", 100);
        // Mode at offset 100
        assert_eq!(&header[100..107], b"0000644");
    }

    #[test]
    fn test_tar_header_size() {
        let header = create_tar_header("test.ovf", 1234);
        // Size at offset 124 (12 bytes octal)
        assert_eq!(&header[124..135], b"00000002322"); // 1234 in octal
    }

    #[test]
    fn test_tar_header_type_flag() {
        let header = create_tar_header("test.ovf", 100);
        // Type flag at offset 156
        assert_eq!(header[156], b'0');
    }

    #[test]
    fn test_tar_header_ustar() {
        let header = create_tar_header("test.ovf", 100);
        // USTAR indicator at offset 257
        assert_eq!(&header[257..263], b"ustar\0");
        assert_eq!(&header[263..265], b"00");
    }

    #[test]
    fn test_ova_tar_structure() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = OvaWriter::new(buffer).unwrap();
        writer.add_file("test.txt", b"hello").unwrap();
        let result = writer.finish().unwrap();

        let data = result.into_inner();

        // First 512 bytes are the header for test.txt
        // Check filename in first 100 bytes
        assert_eq!(&data[0..8], b"test.txt");

        // Check that TAR structure is valid - file content follows header
        assert_eq!(&data[512..517], b"hello");
    }

    #[test]
    fn test_ova_manifest_generation() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = OvaWriter::new(buffer).unwrap();
        writer.add_file("file1.ovf", b"content1").unwrap();
        writer.add_file("file2.vmdk", b"content2").unwrap();
        let result = writer.finish().unwrap();

        let data = result.into_inner();

        // Find manifest.mf in the archive
        let manifest_header_pos = find_file_in_tar(&data, "manifest.mf");
        assert!(manifest_header_pos.is_some());

        let manifest_pos = manifest_header_pos.unwrap() + 512;
        let manifest_content =
            String::from_utf8_lossy(&data[manifest_pos..manifest_pos + 200]).to_string();

        // Check manifest format
        assert!(manifest_content.contains("SHA256(file1.ovf)= "));
        assert!(manifest_content.contains("SHA256(file2.vmdk)= "));
    }

    #[test]
    fn test_ova_streaming_write() {
        let buffer = Cursor::new(Vec::new());
        let mut ova_writer = OvaWriter::new(buffer).unwrap();

        let data = b"streaming content";
        {
            let mut stream_writer = ova_writer
                .add_file_streaming("stream.txt", data.len() as u64)
                .unwrap();
            stream_writer.write_all(data).unwrap();
            stream_writer.finish().unwrap();
        }

        let result = ova_writer.finish().unwrap();
        let archive_data = result.into_inner();

        // Check filename
        assert_eq!(&archive_data[0..10], b"stream.txt");

        // Check content
        assert_eq!(&archive_data[512..512 + data.len()], data);
    }

    #[test]
    fn test_ova_streaming_size_mismatch() {
        let buffer = Cursor::new(Vec::new());
        let mut ova_writer = OvaWriter::new(buffer).unwrap();

        {
            let mut stream_writer = ova_writer.add_file_streaming("test.txt", 100).unwrap();
            stream_writer.write_all(b"short").unwrap();
            let result = stream_writer.finish();
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_ova_padding() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = OvaWriter::new(buffer).unwrap();

        // Add a file with non-512-aligned size
        writer.add_file("test.txt", b"hello").unwrap(); // 5 bytes

        let result = writer.finish().unwrap();
        let data = result.into_inner();

        // Total size should be:
        // - 512 bytes header for test.txt
        // - 512 bytes for content (5 bytes + 507 padding)
        // - 512 bytes header for manifest.mf
        // - 512 bytes for manifest content (padded)
        // - 1024 bytes for TAR end marker
        // All sizes should be 512-aligned
        assert_eq!(data.len() % 512, 0);
    }

    /// Helper function to find a file in a TAR archive.
    fn find_file_in_tar(data: &[u8], filename: &str) -> Option<usize> {
        let mut pos = 0;
        while pos + 512 <= data.len() {
            // Check if this is an empty block (end of archive)
            if data[pos..pos + 512].iter().all(|&b| b == 0) {
                break;
            }

            // Extract filename from header
            let name_end = data[pos..pos + 100]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(100);
            let name = std::str::from_utf8(&data[pos..pos + name_end]).ok()?;

            if name == filename {
                return Some(pos);
            }

            // Parse size to skip to next header
            let size_str = std::str::from_utf8(&data[pos + 124..pos + 135]).ok()?;
            let size = u64::from_str_radix(size_str.trim_matches('\0').trim(), 8).ok()?;

            // Move to next header (header + content + padding)
            let content_blocks = (size + 511) / 512;
            pos += 512 + (content_blocks * 512) as usize;
        }
        None
    }
}
