//! Integration tests for full export pipeline.
//!
//! These tests validate the complete VM export workflow from VMX parsing
//! through OVA generation. They require real VMware VM fixtures to run.
//!
//! # Running Integration Tests
//!
//! By default, these tests are ignored since they require VM fixtures.
//! To run them:
//!
//! ```bash
//! # Run all integration tests (requires fixtures)
//! cargo test --test integration_test -- --ignored
//!
//! # Run a specific test
//! cargo test --test integration_test test_full_export_pipeline -- --ignored
//! ```
//!
//! # Setting Up Test Fixtures
//!
//! Create a test VM directory at `tests/fixtures/test-vm/` containing:
//! - `test.vmx` - VMX configuration file
//! - `test.vmdk` - VMDK descriptor file
//! - `test-flat.vmdk` - Flat extent file (the actual disk data)
//!
//! For a minimal test, you can create a small flat disk:
//! ```bash
//! # Create a 1MB flat disk for testing
//! dd if=/dev/zero of=tests/fixtures/test-vm/test-flat.vmdk bs=1024 count=1024
//! ```

use ovatool_core::{
    export_vm, get_vm_info, CompressionLevel, ExportOptions, ExportPhase, ExportProgress,
};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::NamedTempFile;

/// Path to the test VM fixture directory.
const TEST_VM_DIR: &str = "tests/fixtures/test-vm";

/// Path to the test VMX file.
fn test_vmx_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(TEST_VM_DIR)
        .join("test.vmx")
}

/// Check if the test fixture exists and is complete.
fn fixture_exists() -> bool {
    let vmx_path = test_vmx_path();
    if !vmx_path.exists() {
        return false;
    }

    // Check for VMDK files
    let vm_dir = vmx_path.parent().unwrap();
    let vmdk_path = vm_dir.join("test.vmdk");
    let flat_path = vm_dir.join("test-flat.vmdk");

    vmdk_path.exists() && flat_path.exists()
}

/// Skip test if fixture is not available, with helpful message.
macro_rules! require_fixture {
    () => {
        if !fixture_exists() {
            eprintln!(
                "Skipping test: VM fixture not found.\n\
                 Expected files at {}:\n\
                 - test.vmx\n\
                 - test.vmdk\n\
                 - test-flat.vmdk\n\n\
                 See integration_test.rs module documentation for setup instructions.",
                TEST_VM_DIR
            );
            return;
        }
    };
}

// =============================================================================
// Full Export Pipeline Tests
// =============================================================================

#[test]
#[ignore] // Requires real VMX fixture
fn test_full_export_pipeline() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    let options = ExportOptions::default();
    let result = export_vm(&vmx_path, &output_path, options, None);

    assert!(result.is_ok(), "Export failed: {:?}", result.err());
    assert!(output_path.exists(), "OVA file not created");

    // Verify file has reasonable size (at minimum TAR overhead)
    let metadata = std::fs::metadata(&output_path).unwrap();
    assert!(
        metadata.len() > 512,
        "OVA too small to be valid: {} bytes",
        metadata.len()
    );

    // Verify it's a valid TAR by checking first file header
    let contents = std::fs::read(&output_path).unwrap();
    verify_ova_structure(&contents);

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

#[test]
#[ignore] // Requires real VMX fixture
fn test_export_with_progress_callback() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    // Track progress callbacks
    let callback_count = Arc::new(AtomicUsize::new(0));
    let phases_seen = Arc::new(std::sync::Mutex::new(Vec::new()));

    let callback_count_clone = Arc::clone(&callback_count);
    let phases_seen_clone = Arc::clone(&phases_seen);

    let progress_callback = Box::new(move |progress: ExportProgress| {
        callback_count_clone.fetch_add(1, Ordering::SeqCst);
        let mut phases = phases_seen_clone.lock().unwrap();
        if phases.last() != Some(&progress.phase) {
            phases.push(progress.phase);
        }

        // Verify progress values are reasonable
        assert!(
            progress.percent_complete() >= 0.0 && progress.percent_complete() <= 100.0,
            "Invalid progress percentage: {}",
            progress.percent_complete()
        );
    });

    let options = ExportOptions::default();
    let result = export_vm(&vmx_path, &output_path, options, Some(progress_callback));

    assert!(result.is_ok(), "Export failed: {:?}", result.err());

    // Verify callbacks were invoked
    let count = callback_count.load(Ordering::SeqCst);
    assert!(count > 0, "Progress callback was never invoked");

    // Verify we saw the expected phases in order
    let phases = phases_seen.lock().unwrap();
    assert!(
        phases.contains(&ExportPhase::Parsing),
        "Missing Parsing phase"
    );
    assert!(
        phases.contains(&ExportPhase::Compressing),
        "Missing Compressing phase"
    );
    assert!(
        phases.contains(&ExportPhase::Writing),
        "Missing Writing phase"
    );
    assert!(
        phases.contains(&ExportPhase::Finalizing),
        "Missing Finalizing phase"
    );
    assert!(
        phases.contains(&ExportPhase::Complete),
        "Missing Complete phase"
    );

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

#[test]
#[ignore] // Requires real VMX fixture
fn test_export_creates_valid_manifest() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    let options = ExportOptions::default();
    let result = export_vm(&vmx_path, &output_path, options, None);
    assert!(result.is_ok(), "Export failed: {:?}", result.err());

    let contents = std::fs::read(&output_path).unwrap();

    // Extract manifest from OVA
    let manifest = extract_file_from_tar(&contents, "manifest.mf");
    assert!(manifest.is_some(), "Manifest file not found in OVA");

    let manifest_bytes = manifest.unwrap();
    let manifest_content = String::from_utf8_lossy(&manifest_bytes);

    // Verify manifest format: SHA256(filename)= hash
    assert!(
        manifest_content.contains("SHA256("),
        "Manifest missing SHA256 entries"
    );
    assert!(
        manifest_content.contains(")= "),
        "Manifest has invalid format"
    );

    // Verify manifest contains entries for OVF and VMDK
    assert!(
        manifest_content.contains(".ovf)"),
        "Manifest missing OVF entry"
    );
    assert!(
        manifest_content.contains(".vmdk)"),
        "Manifest missing VMDK entry"
    );

    // Verify SHA256 hashes are 64 hex characters
    for line in manifest_content.lines() {
        if line.contains("SHA256(") {
            let hash_part = line.split("= ").nth(1).unwrap_or("");
            assert_eq!(
                hash_part.len(),
                64,
                "Invalid SHA256 hash length in manifest"
            );
            assert!(
                hash_part.chars().all(|c| c.is_ascii_hexdigit()),
                "Invalid SHA256 hash characters in manifest"
            );
        }
    }

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

// =============================================================================
// Compression Level Tests
// =============================================================================

#[test]
#[ignore] // Requires real VMX fixture
fn test_export_compression_level_fast() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    let options = ExportOptions::fast();
    assert_eq!(options.compression, CompressionLevel::Fast);

    let result = export_vm(&vmx_path, &output_path, options, None);
    assert!(
        result.is_ok(),
        "Export with Fast compression failed: {:?}",
        result.err()
    );
    assert!(output_path.exists(), "OVA file not created");

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

#[test]
#[ignore] // Requires real VMX fixture
fn test_export_compression_level_balanced() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    let options = ExportOptions::default();
    assert_eq!(options.compression, CompressionLevel::Balanced);

    let result = export_vm(&vmx_path, &output_path, options, None);
    assert!(
        result.is_ok(),
        "Export with Balanced compression failed: {:?}",
        result.err()
    );
    assert!(output_path.exists(), "OVA file not created");

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

#[test]
#[ignore] // Requires real VMX fixture
fn test_export_compression_level_max() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    let options = ExportOptions::max_compression();
    assert_eq!(options.compression, CompressionLevel::Max);

    let result = export_vm(&vmx_path, &output_path, options, None);
    assert!(
        result.is_ok(),
        "Export with Max compression failed: {:?}",
        result.err()
    );
    assert!(output_path.exists(), "OVA file not created");

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

// =============================================================================
// OVA Structure Verification Tests
// =============================================================================

#[test]
#[ignore] // Requires real VMX fixture
fn test_ova_structure_verification() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    let options = ExportOptions::default();
    let result = export_vm(&vmx_path, &output_path, options, None);
    assert!(result.is_ok(), "Export failed: {:?}", result.err());

    let contents = std::fs::read(&output_path).unwrap();

    // Extract list of files in the OVA
    let files = extract_tar_filenames(&contents);

    // Verify OVF file is present
    let has_ovf = files.iter().any(|f| f.ends_with(".ovf"));
    assert!(has_ovf, "OVA missing OVF descriptor file");

    // Verify at least one VMDK file is present
    let has_vmdk = files.iter().any(|f| f.ends_with(".vmdk"));
    assert!(has_vmdk, "OVA missing VMDK disk file");

    // Verify manifest is present
    let has_manifest = files.iter().any(|f| f == "manifest.mf");
    assert!(has_manifest, "OVA missing manifest.mf");

    // Verify OVF content is valid XML-like
    let ovf_name = files.iter().find(|f| f.ends_with(".ovf")).unwrap();
    let ovf_content = extract_file_from_tar(&contents, ovf_name);
    assert!(ovf_content.is_some(), "Failed to extract OVF from OVA");

    let ovf_bytes = ovf_content.unwrap();
    let ovf_str = String::from_utf8_lossy(&ovf_bytes);
    assert!(
        ovf_str.contains("<?xml"),
        "OVF does not start with XML declaration"
    );
    assert!(
        ovf_str.contains("ovf:Envelope") || ovf_str.contains("<Envelope"),
        "OVF missing Envelope element"
    );
    assert!(
        ovf_str.contains("</ovf:Envelope>") || ovf_str.contains("</Envelope>"),
        "OVF missing closing Envelope tag"
    );

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

#[test]
#[ignore] // Requires real VMX fixture
fn test_ova_tar_format_compliance() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    let options = ExportOptions::default();
    let result = export_vm(&vmx_path, &output_path, options, None);
    assert!(result.is_ok(), "Export failed: {:?}", result.err());

    let contents = std::fs::read(&output_path).unwrap();

    // TAR file must be a multiple of 512 bytes
    assert_eq!(
        contents.len() % 512,
        0,
        "OVA size is not 512-byte aligned"
    );

    // Must end with two zero blocks (1024 bytes)
    let end_blocks = &contents[contents.len() - 1024..];
    assert!(
        end_blocks.iter().all(|&b| b == 0),
        "OVA missing proper TAR end marker"
    );

    // Verify USTAR format in first header
    assert_eq!(
        &contents[257..263],
        b"ustar\0",
        "OVA not in USTAR format"
    );

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

// =============================================================================
// VM Info Tests
// =============================================================================

#[test]
#[ignore] // Requires real VMX fixture
fn test_get_vm_info() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let info = get_vm_info(&vmx_path).unwrap();

    // Verify basic VM info is populated
    assert!(!info.name.is_empty(), "VM name should not be empty");
    assert!(info.memory_mb > 0, "Memory should be greater than 0");
    assert!(info.cpus > 0, "CPUs should be greater than 0");
    assert!(!info.disks.is_empty(), "VM should have at least one disk");

    // Verify disk info
    for disk in &info.disks {
        assert!(!disk.filename.is_empty(), "Disk filename should not be empty");
        assert!(!disk.create_type.is_empty(), "Disk create_type should not be empty");
    }
}

#[test]
#[ignore] // Requires real VMX fixture
fn test_get_vm_info_disk_details() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let info = get_vm_info(&vmx_path).unwrap();

    // Verify total disk size calculation
    let calculated_total: u64 = info.disks.iter().map(|d| d.size_bytes).sum();
    assert_eq!(
        calculated_total, info.total_disk_size,
        "Total disk size mismatch"
    );

    // Verify disk sizes are reasonable (at least 1 sector)
    for disk in &info.disks {
        assert!(
            disk.size_bytes >= 512 || disk.size_bytes == 0,
            "Disk size should be at least one sector or zero if unknown"
        );
    }
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_export_nonexistent_vmx() {
    let vmx_path = Path::new("/nonexistent/path/to/vm.vmx");
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    let options = ExportOptions::default();
    let result = export_vm(vmx_path, &output_path, options, None);

    assert!(result.is_err(), "Export should fail for nonexistent VMX");
}

#[test]
fn test_get_vm_info_nonexistent_vmx() {
    let vmx_path = Path::new("/nonexistent/path/to/vm.vmx");
    let result = get_vm_info(vmx_path);

    assert!(result.is_err(), "get_vm_info should fail for nonexistent VMX");
}

// =============================================================================
// Custom Export Options Tests
// =============================================================================

#[test]
#[ignore] // Requires real VMX fixture
fn test_export_custom_chunk_size() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    // Use smaller chunk size
    let options = ExportOptions::new(
        CompressionLevel::Balanced,
        1024 * 1024, // 1 MB chunks
        0,           // auto threads
    );

    let result = export_vm(&vmx_path, &output_path, options, None);
    assert!(
        result.is_ok(),
        "Export with custom chunk size failed: {:?}",
        result.err()
    );

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

#[test]
#[ignore] // Requires real VMX fixture
fn test_export_explicit_thread_count() {
    require_fixture!();

    let vmx_path = test_vmx_path();
    let output = NamedTempFile::new().unwrap();
    let output_path = output.path().with_extension("ova");

    // Use explicit thread count
    let options = ExportOptions::new(
        CompressionLevel::Balanced,
        64 * 1024 * 1024, // 64 MB chunks
        2,                // 2 threads
    );

    let result = export_vm(&vmx_path, &output_path, options, None);
    assert!(
        result.is_ok(),
        "Export with explicit threads failed: {:?}",
        result.err()
    );

    // Cleanup
    let _ = std::fs::remove_file(&output_path);
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Verify OVA has valid TAR structure with expected file types.
fn verify_ova_structure(data: &[u8]) {
    // Must be at least one header block + end marker
    assert!(data.len() >= 512 + 1024, "OVA too small");

    // Check first filename ends with expected extension
    let name_end = data[0..100].iter().position(|&b| b == 0).unwrap_or(100);
    let first_filename = std::str::from_utf8(&data[0..name_end]).unwrap();

    // First file should be either OVF or VMDK
    assert!(
        first_filename.ends_with(".ovf") || first_filename.ends_with(".vmdk"),
        "First file in OVA should be .ovf or .vmdk, got: {}",
        first_filename
    );
}

/// Extract all filenames from a TAR archive.
fn extract_tar_filenames(data: &[u8]) -> Vec<String> {
    let mut filenames = Vec::new();
    let mut pos = 0;

    while pos + 512 <= data.len() {
        // Check for end of archive (zero block)
        if data[pos..pos + 512].iter().all(|&b| b == 0) {
            break;
        }

        // Extract filename (first 100 bytes, null-terminated)
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

/// Extract file content from a TAR archive by filename.
fn extract_file_from_tar(data: &[u8], filename: &str) -> Option<Vec<u8>> {
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
        let name = std::str::from_utf8(&data[pos..pos + name_end]).ok()?;

        // Parse size
        let size_str = std::str::from_utf8(&data[pos + 124..pos + 135]).ok()?;
        let size = u64::from_str_radix(size_str.trim_matches('\0').trim(), 8).ok()?;

        if name == filename {
            let content_start = pos + 512;
            let content_end = content_start + size as usize;
            return Some(data[content_start..content_end].to_vec());
        }

        // Move to next header
        let content_blocks = (size + 511) / 512;
        pos += 512 + (content_blocks * 512) as usize;
    }

    None
}

// =============================================================================
// Tests That Don't Require Fixtures
// =============================================================================

#[test]
fn test_export_options_defaults() {
    let options = ExportOptions::default();
    assert_eq!(options.compression, CompressionLevel::Balanced);
    assert_eq!(options.chunk_size, 64 * 1024 * 1024); // 64 MB
    assert_eq!(options.num_threads, 0); // auto
}

#[test]
fn test_export_options_fast() {
    let options = ExportOptions::fast();
    assert_eq!(options.compression, CompressionLevel::Fast);
}

#[test]
fn test_export_options_max_compression() {
    let options = ExportOptions::max_compression();
    assert_eq!(options.compression, CompressionLevel::Max);
}

#[test]
fn test_export_phase_display() {
    assert_eq!(format!("{}", ExportPhase::Parsing), "Parsing");
    assert_eq!(format!("{}", ExportPhase::Compressing), "Compressing");
    assert_eq!(format!("{}", ExportPhase::Writing), "Writing");
    assert_eq!(format!("{}", ExportPhase::Finalizing), "Finalizing");
    assert_eq!(format!("{}", ExportPhase::Complete), "Complete");
}

#[test]
fn test_export_progress_percent() {
    let mut progress = ExportProgress {
        phase: ExportPhase::Compressing,
        bytes_processed: 500,
        bytes_total: 1000,
        current_disk: 1,
        total_disks: 1,
    };

    assert_eq!(progress.percent_complete(), 50.0);

    progress.bytes_processed = 1000;
    assert_eq!(progress.percent_complete(), 100.0);

    progress.bytes_processed = 0;
    assert_eq!(progress.percent_complete(), 0.0);
}

#[test]
fn test_export_progress_zero_total() {
    let progress = ExportProgress {
        phase: ExportPhase::Parsing,
        bytes_processed: 0,
        bytes_total: 0,
        current_disk: 0,
        total_disks: 0,
    };

    // Zero total should return 0% (not NaN or panic)
    assert_eq!(progress.percent_complete(), 0.0);
}

#[test]
fn test_compression_level_values() {
    assert_eq!(CompressionLevel::Fast.to_zlib_level(), 1);
    assert_eq!(CompressionLevel::Balanced.to_zlib_level(), 6);
    assert_eq!(CompressionLevel::Max.to_zlib_level(), 9);
}
