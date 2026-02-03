//! OVATool CLI - Export VMware VMs to OVA format.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    },

    /// Display information about a VMware VM.
    Info {
        /// Path to the VMX file.
        vmx_file: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Export { vmx_file, output } => {
            export_vm(&vmx_file, output.as_deref())?;
        }
        Commands::Info { vmx_file } => {
            show_info(&vmx_file)?;
        }
    }

    Ok(())
}

fn export_vm(vmx_file: &std::path::Path, output: Option<&std::path::Path>) -> Result<()> {
    todo!("Export functionality not yet implemented")
}

fn show_info(vmx_file: &std::path::Path) -> Result<()> {
    todo!("Info functionality not yet implemented")
}
