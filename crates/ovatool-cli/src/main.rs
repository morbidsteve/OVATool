//! OVATool CLI - Export VMware VMs to OVA format.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use ovatool_core::{
    export_vm, get_vm_info, CompressionLevel, ExportOptions, ExportPhase, ExportProgress,
};

/// Fast, multithreaded tool for exporting VMware VMs to OVA format.
#[derive(Parser)]
#[command(name = "ovatool")]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Export a VMware VM to OVA format.
    Export {
        /// Path to the VMX file.
        vmx_file: PathBuf,

        /// Output OVA file path. Defaults to the VM name with .ova extension.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Compression level (fast, balanced, max).
        #[arg(short, long, value_enum, default_value = "balanced")]
        compression: CompressionArg,

        /// Number of threads to use (0 = auto-detect).
        #[arg(short, long, default_value = "0")]
        threads: usize,

        /// Chunk size in megabytes for processing.
        #[arg(long, default_value = "64")]
        chunk_size: usize,

        /// Suppress progress output.
        #[arg(short, long)]
        quiet: bool,
    },

    /// Display information about a VMware VM.
    Info {
        /// Path to the VMX file.
        vmx_file: PathBuf,
    },
}

/// Compression level argument mapping.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum CompressionArg {
    /// Fast compression (zlib level 1).
    Fast,
    /// Balanced compression (zlib level 6).
    Balanced,
    /// Maximum compression (zlib level 9).
    Max,
}

impl From<CompressionArg> for CompressionLevel {
    fn from(arg: CompressionArg) -> Self {
        match arg {
            CompressionArg::Fast => CompressionLevel::Fast,
            CompressionArg::Balanced => CompressionLevel::Balanced,
            CompressionArg::Max => CompressionLevel::Max,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Export {
            vmx_file,
            output,
            compression,
            threads,
            chunk_size,
            quiet,
        } => {
            run_export(&vmx_file, output.as_deref(), compression, threads, chunk_size, quiet)?;
        }
        Commands::Info { vmx_file } => {
            show_info(&vmx_file)?;
        }
    }

    Ok(())
}

fn run_export(
    vmx_file: &std::path::Path,
    output: Option<&std::path::Path>,
    compression: CompressionArg,
    threads: usize,
    chunk_size_mb: usize,
    quiet: bool,
) -> Result<()> {
    // Get VM info first to show details and determine output path
    let vm_info = get_vm_info(vmx_file)?;

    // Determine output path
    let output_path = match output {
        Some(path) => path.to_path_buf(),
        None => {
            let sanitized_name = sanitize_filename(&vm_info.name);
            PathBuf::from(format!("{}.ova", sanitized_name))
        }
    };

    if !quiet {
        println!("VM Export");
        println!("---------");
        println!("Name:      {}", vm_info.name);
        println!("Guest OS:  {}", vm_info.guest_os);
        println!("CPUs:      {}", vm_info.cpus);
        println!("Memory:    {} MB", vm_info.memory_mb);
        println!("Disks:     {}", vm_info.disks.len());
        println!(
            "Total:     {}",
            format_bytes(vm_info.total_disk_size)
        );
        println!();
        println!("Output:    {}", output_path.display());
        println!(
            "Compression: {:?}",
            CompressionLevel::from(compression)
        );
        println!();
    }

    // Create export options
    let chunk_size_bytes = chunk_size_mb * 1024 * 1024;
    let options = ExportOptions::new(compression.into(), chunk_size_bytes, threads);

    // Set up progress tracking
    let progress_bar: Option<Arc<Mutex<ProgressBar>>> = if quiet {
        None
    } else {
        let pb = ProgressBar::new(vm_info.total_disk_size);
        let style = ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")?
            .progress_chars("#>-");
        pb.set_style(style);
        Some(Arc::new(Mutex::new(pb)))
    };

    // Create progress callback
    let callback: Option<ovatool_core::ProgressCallback> = if let Some(pb_arc) = progress_bar.clone() {
        Some(Box::new(move |progress: ExportProgress| {
            let pb = pb_arc.lock().unwrap();
            pb.set_position(progress.bytes_processed);

            // Update message based on phase
            let phase_msg = match progress.phase {
                ExportPhase::Parsing => "Parsing...",
                ExportPhase::Compressing => {
                    if progress.total_disks > 1 {
                        "Compressing disk"
                    } else {
                        "Compressing..."
                    }
                }
                ExportPhase::Writing => "Writing...",
                ExportPhase::Finalizing => "Finalizing...",
                ExportPhase::Complete => "Complete!",
            };
            pb.set_message(phase_msg.to_string());
        }))
    } else {
        None
    };

    // Run the export
    export_vm(vmx_file, &output_path, options, callback)?;

    // Finish progress bar
    if let Some(pb_arc) = progress_bar {
        let pb = pb_arc.lock().unwrap();
        pb.finish_with_message("Complete!");
    }

    if !quiet {
        println!();
        println!("Export completed successfully: {}", output_path.display());

        // Show output file size
        if let Ok(metadata) = std::fs::metadata(&output_path) {
            println!(
                "Output size: {} (compression ratio: {:.1}%)",
                format_bytes(metadata.len()),
                (metadata.len() as f64 / vm_info.total_disk_size as f64) * 100.0
            );
        }
    }

    Ok(())
}

fn show_info(vmx_file: &std::path::Path) -> Result<()> {
    let vm_info = get_vm_info(vmx_file)?;

    println!("VM Information");
    println!("==============");
    println!();
    println!("Name:      {}", vm_info.name);
    println!("Guest OS:  {}", vm_info.guest_os);
    println!("CPUs:      {}", vm_info.cpus);
    println!("Memory:    {} MB", vm_info.memory_mb);
    println!();

    if vm_info.disks.is_empty() {
        println!("Disks:     None");
    } else {
        println!("Disks:");
        for (i, disk) in vm_info.disks.iter().enumerate() {
            println!(
                "  {}. {} - {} ({})",
                i + 1,
                disk.filename,
                format_bytes(disk.size_bytes),
                disk.create_type
            );
        }
        println!();
        println!(
            "Total disk size: {}",
            format_bytes(vm_info.total_disk_size)
        );
    }

    Ok(())
}

/// Format bytes as human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
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
