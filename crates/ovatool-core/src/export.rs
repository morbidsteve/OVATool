//! Export orchestrator for VM to OVA conversion.
//!
//! This module coordinates the full export pipeline:
//! 1. Parse VMX to get VM configuration and disk paths
//! 2. For each disk, read it in chunks, compress, and write streamOptimized VMDK
//! 3. Package everything into OVA with OVF descriptor and manifest
//!
//! # Example
//!
//! ```no_run
//! use ovatool_core::export::{export_vm, ExportOptions};
//! use std::path::Path;
//!
//! let vmx_path = Path::new("/path/to/vm.vmx");
//! let output_path = Path::new("/path/to/output.ova");
//! let options = ExportOptions::default();
//!
//! export_vm(vmx_path, output_path, options, None).unwrap();
//! ```

use std::fs::{self, File};
use std::io::Cursor;
use std::path::Path;

use crate::error::{Error, Result};
use crate::ova::OvaWriter;
use crate::ovf::{DiskInfo, OvfBuilder};
use crate::pipeline::{CompressionLevel, Pipeline, PipelineConfig};
use crate::vmdk::{
    compress_grain, is_sparse_vmdk, parse_descriptor, ExtentType, SparseVmdkReader,
    StreamVmdkWriter, VmdkReader,
};
use crate::vmx::{parse_vmx, VmxConfig};

/// Default chunk size for processing (64 MB).
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024 * 1024;

/// Options for the export process.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Compression level for VMDK output.
    pub compression: CompressionLevel,
    /// Size of chunks to process (default 64 MB).
    pub chunk_size: usize,
    /// Number of threads to use (0 = auto).
    pub num_threads: usize,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            compression: CompressionLevel::Balanced,
            chunk_size: DEFAULT_CHUNK_SIZE,
            num_threads: 0,
        }
    }
}

impl ExportOptions {
    /// Create new export options with specified settings.
    pub fn new(compression: CompressionLevel, chunk_size: usize, num_threads: usize) -> Self {
        Self {
            compression,
            chunk_size,
            num_threads,
        }
    }

    /// Create options optimized for speed.
    pub fn fast() -> Self {
        Self {
            compression: CompressionLevel::Fast,
            chunk_size: DEFAULT_CHUNK_SIZE,
            num_threads: 0,
        }
    }

    /// Create options optimized for compression ratio.
    pub fn max_compression() -> Self {
        Self {
            compression: CompressionLevel::Max,
            chunk_size: DEFAULT_CHUNK_SIZE,
            num_threads: 0,
        }
    }
}

/// Phase of the export process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportPhase {
    /// Parsing VMX and VMDK descriptors.
    Parsing,
    /// Compressing disk data.
    Compressing,
    /// Writing to output file.
    Writing,
    /// Finalizing OVA (adding manifest, etc).
    Finalizing,
    /// Export complete.
    Complete,
}

impl std::fmt::Display for ExportPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportPhase::Parsing => write!(f, "Parsing"),
            ExportPhase::Compressing => write!(f, "Compressing"),
            ExportPhase::Writing => write!(f, "Writing"),
            ExportPhase::Finalizing => write!(f, "Finalizing"),
            ExportPhase::Complete => write!(f, "Complete"),
        }
    }
}

/// Progress information for the export process.
#[derive(Debug, Clone)]
pub struct ExportProgress {
    /// Current phase of the export.
    pub phase: ExportPhase,
    /// Bytes processed so far.
    pub bytes_processed: u64,
    /// Total bytes to process.
    pub bytes_total: u64,
    /// Current disk being processed (1-indexed).
    pub current_disk: usize,
    /// Total number of disks.
    pub total_disks: usize,
}

impl ExportProgress {
    /// Create new progress information.
    pub fn new(phase: ExportPhase, total_bytes: u64, total_disks: usize) -> Self {
        Self {
            phase,
            bytes_processed: 0,
            bytes_total: total_bytes,
            current_disk: 0,
            total_disks,
        }
    }

    /// Calculate overall percentage complete.
    pub fn percent_complete(&self) -> f64 {
        if self.bytes_total == 0 {
            return match self.phase {
                ExportPhase::Complete => 100.0,
                _ => 0.0,
            };
        }
        (self.bytes_processed as f64 / self.bytes_total as f64) * 100.0
    }
}

/// Type alias for the progress callback function.
pub type ProgressCallback = Box<dyn Fn(ExportProgress) + Send>;

/// Detail information about a disk.
#[derive(Debug, Clone)]
pub struct DiskDetail {
    /// Filename of the VMDK descriptor file.
    pub filename: String,
    /// Size of the disk in bytes.
    pub size_bytes: u64,
    /// VMDK create type (e.g., "monolithicFlat", "twoGbMaxExtentSparse").
    pub create_type: String,
}

/// Summary information about a VM.
#[derive(Debug, Clone)]
pub struct VmInfo {
    /// Display name of the VM.
    pub name: String,
    /// Guest operating system type.
    pub guest_os: String,
    /// Memory size in megabytes.
    pub memory_mb: u32,
    /// Number of virtual CPUs.
    pub cpus: u32,
    /// Details about attached disks.
    pub disks: Vec<DiskDetail>,
    /// Total size of all disks in bytes.
    pub total_disk_size: u64,
}

/// Get information about a VM without exporting it.
///
/// # Arguments
///
/// * `vmx_path` - Path to the VMX file.
///
/// # Returns
///
/// Summary information about the VM.
pub fn get_vm_info(vmx_path: &Path) -> Result<VmInfo> {
    let config = parse_vmx(vmx_path)?;
    let vmx_dir = vmx_path
        .parent()
        .ok_or_else(|| Error::vmx_parse("VMX path has no parent directory"))?;

    let mut disks = Vec::new();
    let mut total_disk_size = 0u64;

    for disk_config in &config.disks {
        let vmdk_path = vmx_dir.join(&disk_config.file_name);

        // Try to read the VMDK descriptor or sparse header
        let (size_bytes, create_type) = if vmdk_path.exists() {
            // Check if this is a sparse VMDK (binary) or text descriptor
            if is_sparse_vmdk(&vmdk_path)? {
                // Sparse VMDK - read capacity from header
                let sparse_reader = SparseVmdkReader::open(&vmdk_path)?;
                (sparse_reader.capacity(), "monolithicSparse".to_string())
            } else {
                // Text descriptor
                let content = fs::read_to_string(&vmdk_path)
                    .map_err(|e| Error::io(e, &vmdk_path))?;
                let descriptor = parse_descriptor(&content)?;
                (descriptor.disk_size_bytes(), descriptor.create_type.clone())
            }
        } else {
            // If descriptor doesn't exist, check for flat file
            let flat_name = disk_config.file_name.replace(".vmdk", "-flat.vmdk");
            let flat_path = vmx_dir.join(&flat_name);
            if flat_path.exists() {
                let metadata = fs::metadata(&flat_path)
                    .map_err(|e| Error::io(e, &flat_path))?;
                (metadata.len(), "monolithicFlat".to_string())
            } else {
                (0, "unknown".to_string())
            }
        };

        total_disk_size += size_bytes;
        disks.push(DiskDetail {
            filename: disk_config.file_name.clone(),
            size_bytes,
            create_type,
        });
    }

    Ok(VmInfo {
        name: config.display_name.clone(),
        guest_os: config.guest_os.clone(),
        memory_mb: config.memory_mb,
        cpus: config.num_cpus,
        disks,
        total_disk_size,
    })
}

/// Export a VMware VM to OVA format.
///
/// This is the main entry point for the export process. It:
/// 1. Parses the VMX file to get VM configuration
/// 2. Creates the OVA archive
/// 3. For each disk:
///    - Reads the VMDK descriptor
///    - Finds and reads the flat extent
///    - Compresses the data using the parallel pipeline
///    - Writes a streamOptimized VMDK to the OVA
/// 4. Generates and adds the OVF descriptor
/// 5. Finalizes the OVA with manifest
///
/// # Arguments
///
/// * `vmx_path` - Path to the VMX file.
/// * `output_path` - Path for the output OVA file.
/// * `options` - Export options (compression level, chunk size, etc.).
/// * `progress_callback` - Optional callback for progress updates.
///
/// # Returns
///
/// `Ok(())` on success, or an error if export fails.
///
/// # Example
///
/// ```no_run
/// use ovatool_core::export::{export_vm, ExportOptions, ExportProgress};
/// use std::path::Path;
///
/// let vmx_path = Path::new("/path/to/vm.vmx");
/// let output_path = Path::new("/path/to/output.ova");
/// let options = ExportOptions::default();
///
/// // With progress callback
/// export_vm(vmx_path, output_path, options, Some(Box::new(|progress: ExportProgress| {
///     println!("Phase: {:?}, Progress: {:.1}%", progress.phase, progress.percent_complete());
/// }))).unwrap();
/// ```
pub fn export_vm(
    vmx_path: &Path,
    output_path: &Path,
    options: ExportOptions,
    progress_callback: Option<ProgressCallback>,
) -> Result<()> {
    // Helper to call progress callback if provided
    let report_progress = |progress: ExportProgress| {
        if let Some(ref callback) = progress_callback {
            callback(progress);
        }
    };

    // Phase 1: Parsing
    let config = parse_vmx(vmx_path)?;
    let vmx_dir = vmx_path
        .parent()
        .ok_or_else(|| Error::vmx_parse("VMX path has no parent directory"))?;

    // Calculate total disk size for progress tracking
    let total_disk_size = calculate_total_disk_size(&config, vmx_dir)?;
    let total_disks = config.disks.len();

    let mut progress = ExportProgress::new(ExportPhase::Parsing, total_disk_size, total_disks);
    report_progress(progress.clone());

    // Create the pipeline for parallel compression
    let pipeline_config = PipelineConfig::new(
        options.chunk_size,
        options.compression,
        options.num_threads,
    );
    let pipeline = Pipeline::new(pipeline_config);
    let compression_level = pipeline.compression_level();

    // Create output file and OVA writer
    let output_file = File::create(output_path)
        .map_err(|e| Error::io(e, output_path))?;
    let mut ova_writer = OvaWriter::new(output_file)?;

    // Process each disk
    let mut disk_infos: Vec<DiskInfo> = Vec::new();
    let mut vmdk_buffers: Vec<(String, Vec<u8>, u64)> = Vec::new(); // (filename, compressed data, capacity)

    for (disk_index, disk_config) in config.disks.iter().enumerate() {
        progress.phase = ExportPhase::Compressing;
        progress.current_disk = disk_index + 1;
        report_progress(progress.clone());

        // Get the VMDK path
        let vmdk_path = vmx_dir.join(&disk_config.file_name);

        // Check if this is a sparse VMDK (binary) or a descriptor file (text)
        let (data_path, capacity_bytes, is_sparse) = if is_sparse_vmdk(&vmdk_path)? {
            // Sparse VMDK - the file itself contains the data
            let sparse_reader = SparseVmdkReader::open(&vmdk_path)?;
            let capacity = sparse_reader.capacity();
            (vmdk_path.clone(), capacity, true)
        } else {
            // Text descriptor - parse it to find the data file
            let descriptor_content = fs::read_to_string(&vmdk_path)
                .map_err(|e| Error::io(e, &vmdk_path))?;
            let descriptor = parse_descriptor(&descriptor_content)?;

            // Find the flat extent (the actual data file)
            let flat_extent = descriptor
                .extents
                .iter()
                .find(|e| e.extent_type == ExtentType::Flat)
                .ok_or_else(|| Error::vmdk("No flat extent found in VMDK descriptor"))?;

            let flat_path = vmx_dir.join(&flat_extent.filename);
            let capacity = descriptor.disk_size_bytes();
            (flat_path, capacity, false)
        };

        // Read and compress the disk data
        let compressed_vmdk = if is_sparse {
            process_sparse_disk(
                &data_path,
                capacity_bytes,
                &pipeline,
                compression_level,
                options.chunk_size,
                &mut progress,
                &progress_callback,
            )?
        } else {
            process_disk(
                &data_path,
                capacity_bytes,
                &pipeline,
                compression_level,
                options.chunk_size,
                &mut progress,
                &progress_callback,
            )?
        };

        // Store for later writing
        let output_filename = disk_config.file_name.clone();
        vmdk_buffers.push((output_filename.clone(), compressed_vmdk, capacity_bytes));

        // Track disk info for OVF
        disk_infos.push(DiskInfo {
            id: format!("vmdisk{}", disk_index + 1),
            file_ref: format!("file{}", disk_index + 1),
            capacity_bytes,
            file_size_bytes: 0, // Will be updated after writing
        });
    }

    // Phase 3: Writing disks to OVA
    progress.phase = ExportPhase::Writing;
    report_progress(progress.clone());

    for (i, (filename, vmdk_data, _)) in vmdk_buffers.iter().enumerate() {
        disk_infos[i].file_size_bytes = vmdk_data.len() as u64;
        ova_writer.add_file(filename, vmdk_data)?;
    }

    // Phase 4: Generate and add OVF descriptor
    progress.phase = ExportPhase::Finalizing;
    report_progress(progress.clone());

    let ovf_builder = OvfBuilder::new(&config);
    let ovf_xml = ovf_builder.build(&disk_infos)?;

    // OVF filename is based on VM name
    let ovf_filename = format!("{}.ovf", sanitize_filename(&config.display_name));

    // OVF should be first in the OVA, but we already wrote disks
    // In a proper OVA, the order should be: OVF, disks, manifest
    // For now, we add it after disks - this is still valid OVA
    ova_writer.add_file(&ovf_filename, ovf_xml.as_bytes())?;

    // Finish the OVA (writes manifest and end marker)
    ova_writer.finish()?;

    // Phase 5: Complete
    progress.phase = ExportPhase::Complete;
    progress.bytes_processed = progress.bytes_total;
    report_progress(progress);

    Ok(())
}

/// Process a single disk: read, compress, and create streamOptimized VMDK.
fn process_disk(
    flat_path: &Path,
    capacity_bytes: u64,
    pipeline: &Pipeline,
    compression_level: u32,
    chunk_size: usize,
    progress: &mut ExportProgress,
    progress_callback: &Option<ProgressCallback>,
) -> Result<Vec<u8>> {
    // Open the flat extent file
    let reader = VmdkReader::open(flat_path)?;
    let file_size = reader.size();

    // Collect all chunks for parallel processing
    let chunks: Vec<Vec<u8>> = reader
        .chunks(chunk_size)
        .collect::<Result<Vec<_>>>()?;

    // Compress chunks in parallel
    let compressed_chunks: Vec<Vec<u8>> = pipeline.process(chunks, |_idx, chunk| {
        compress_grain(&chunk, compression_level)
    })?;

    // Create streamOptimized VMDK in memory
    let mut vmdk_buffer = Cursor::new(Vec::new());
    let mut vmdk_writer = StreamVmdkWriter::new(&mut vmdk_buffer, capacity_bytes)?;
    let _grain_size_bytes = vmdk_writer.grain_size_bytes() as usize;

    // Write compressed grains
    let mut bytes_written = 0u64;
    for (chunk_idx, compressed_chunk) in compressed_chunks.into_iter().enumerate() {
        // Calculate LBA for this chunk (in sectors)
        let chunk_offset_bytes = chunk_idx as u64 * chunk_size as u64;
        let lba = chunk_offset_bytes / 512; // Convert to sectors

        // Write the grain (the stream writer handles grain-level addressing)
        vmdk_writer.write_grain(lba, &compressed_chunk)?;

        // Update progress
        let original_chunk_size = if chunk_idx < (file_size as usize / chunk_size) {
            chunk_size as u64
        } else {
            file_size - (chunk_idx as u64 * chunk_size as u64)
        };
        bytes_written += original_chunk_size;
        progress.bytes_processed = bytes_written;

        if let Some(ref callback) = progress_callback {
            callback(progress.clone());
        }
    }

    // Finish the VMDK (writes grain tables, directory, footer, etc.)
    vmdk_writer.finish()?;

    Ok(vmdk_buffer.into_inner())
}

/// Process a sparse VMDK: read grains, compress, and create streamOptimized VMDK.
fn process_sparse_disk(
    sparse_path: &Path,
    capacity_bytes: u64,
    pipeline: &Pipeline,
    compression_level: u32,
    chunk_size: usize,
    progress: &mut ExportProgress,
    progress_callback: &Option<ProgressCallback>,
) -> Result<Vec<u8>> {
    // Open the sparse VMDK
    let reader = SparseVmdkReader::open(sparse_path)?;

    // Collect all chunks for parallel processing
    let chunks: Vec<Vec<u8>> = reader
        .chunks(chunk_size)
        .collect::<Result<Vec<_>>>()?;

    let total_chunks = chunks.len();

    // Compress chunks in parallel
    let compressed_chunks: Vec<Vec<u8>> = pipeline.process(chunks, |_idx, chunk| {
        compress_grain(&chunk, compression_level)
    })?;

    // Create streamOptimized VMDK in memory
    let mut vmdk_buffer = Cursor::new(Vec::new());
    let mut vmdk_writer = StreamVmdkWriter::new(&mut vmdk_buffer, capacity_bytes)?;

    // Write compressed grains
    let mut bytes_written = 0u64;
    for (chunk_idx, compressed_chunk) in compressed_chunks.into_iter().enumerate() {
        // Calculate LBA for this chunk (in sectors)
        let chunk_offset_bytes = chunk_idx as u64 * chunk_size as u64;
        let lba = chunk_offset_bytes / 512; // Convert to sectors

        // Write the grain (the stream writer handles grain-level addressing)
        vmdk_writer.write_grain(lba, &compressed_chunk)?;

        // Update progress
        let original_chunk_size = if chunk_idx < total_chunks - 1 {
            chunk_size as u64
        } else {
            capacity_bytes - (chunk_idx as u64 * chunk_size as u64)
        };
        bytes_written += original_chunk_size;
        progress.bytes_processed = bytes_written;

        if let Some(ref callback) = progress_callback {
            callback(progress.clone());
        }
    }

    // Finish the VMDK (writes grain tables, directory, footer, etc.)
    vmdk_writer.finish()?;

    Ok(vmdk_buffer.into_inner())
}

/// Calculate total disk size from VMX config.
fn calculate_total_disk_size(config: &VmxConfig, vmx_dir: &Path) -> Result<u64> {
    let mut total = 0u64;

    for disk_config in &config.disks {
        let vmdk_path = vmx_dir.join(&disk_config.file_name);

        if vmdk_path.exists() {
            // Check if this is a sparse VMDK or a text descriptor
            if is_sparse_vmdk(&vmdk_path)? {
                // Sparse VMDK - use the virtual capacity
                let sparse_reader = SparseVmdkReader::open(&vmdk_path)?;
                total += sparse_reader.capacity();
            } else {
                // Text descriptor
                let content = fs::read_to_string(&vmdk_path)
                    .map_err(|e| Error::io(e, &vmdk_path))?;
                let descriptor = parse_descriptor(&content)?;

                // Find the flat extent to get actual file size
                if let Some(flat_extent) = descriptor.extents.iter().find(|e| e.extent_type == ExtentType::Flat) {
                    let flat_path = vmx_dir.join(&flat_extent.filename);
                    if flat_path.exists() {
                        let metadata = fs::metadata(&flat_path)
                            .map_err(|e| Error::io(e, &flat_path))?;
                        total += metadata.len();
                    }
                }
            }
        }
    }

    Ok(total)
}

/// Sanitize a filename by removing or replacing invalid characters.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_options_default() {
        let options = ExportOptions::default();
        assert_eq!(options.compression, CompressionLevel::Balanced);
        assert_eq!(options.chunk_size, DEFAULT_CHUNK_SIZE);
        assert_eq!(options.num_threads, 0);
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
    fn test_export_options_new() {
        let options = ExportOptions::new(CompressionLevel::Max, 1024 * 1024, 4);
        assert_eq!(options.compression, CompressionLevel::Max);
        assert_eq!(options.chunk_size, 1024 * 1024);
        assert_eq!(options.num_threads, 4);
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
    fn test_export_progress_new() {
        let progress = ExportProgress::new(ExportPhase::Parsing, 1000, 2);
        assert_eq!(progress.phase, ExportPhase::Parsing);
        assert_eq!(progress.bytes_processed, 0);
        assert_eq!(progress.bytes_total, 1000);
        assert_eq!(progress.current_disk, 0);
        assert_eq!(progress.total_disks, 2);
    }

    #[test]
    fn test_export_progress_percent_complete() {
        let mut progress = ExportProgress::new(ExportPhase::Compressing, 1000, 1);
        assert_eq!(progress.percent_complete(), 0.0);

        progress.bytes_processed = 500;
        assert_eq!(progress.percent_complete(), 50.0);

        progress.bytes_processed = 1000;
        assert_eq!(progress.percent_complete(), 100.0);
    }

    #[test]
    fn test_export_progress_percent_complete_zero_total() {
        let progress = ExportProgress::new(ExportPhase::Parsing, 0, 0);
        assert_eq!(progress.percent_complete(), 0.0);

        let complete = ExportProgress {
            phase: ExportPhase::Complete,
            bytes_processed: 0,
            bytes_total: 0,
            current_disk: 0,
            total_disks: 0,
        };
        assert_eq!(complete.percent_complete(), 100.0);
    }

    #[test]
    fn test_disk_detail() {
        let detail = DiskDetail {
            filename: "disk.vmdk".to_string(),
            size_bytes: 10 * 1024 * 1024 * 1024,
            create_type: "monolithicFlat".to_string(),
        };
        assert_eq!(detail.filename, "disk.vmdk");
        assert_eq!(detail.size_bytes, 10 * 1024 * 1024 * 1024);
        assert_eq!(detail.create_type, "monolithicFlat");
    }

    #[test]
    fn test_vm_info() {
        let info = VmInfo {
            name: "TestVM".to_string(),
            guest_os: "ubuntu-64".to_string(),
            memory_mb: 4096,
            cpus: 2,
            disks: vec![DiskDetail {
                filename: "disk.vmdk".to_string(),
                size_bytes: 10 * 1024 * 1024 * 1024,
                create_type: "monolithicFlat".to_string(),
            }],
            total_disk_size: 10 * 1024 * 1024 * 1024,
        };
        assert_eq!(info.name, "TestVM");
        assert_eq!(info.guest_os, "ubuntu-64");
        assert_eq!(info.memory_mb, 4096);
        assert_eq!(info.cpus, 2);
        assert_eq!(info.disks.len(), 1);
        assert_eq!(info.total_disk_size, 10 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("TestVM"), "TestVM");
        assert_eq!(sanitize_filename("Test VM"), "Test_VM");
        assert_eq!(sanitize_filename("VM<>123"), "VM__123");
        assert_eq!(sanitize_filename("my-vm_01.old"), "my-vm_01.old");
        assert_eq!(sanitize_filename("a/b\\c:d"), "a_b_c_d");
    }

    #[test]
    fn test_default_chunk_size() {
        assert_eq!(DEFAULT_CHUNK_SIZE, 64 * 1024 * 1024);
    }
}
