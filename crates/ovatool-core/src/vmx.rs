//! VMX file parsing.
//!
//! This module handles parsing VMware VMX configuration files to extract
//! VM metadata and disk references.

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Configuration for a virtual disk attached to the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskConfig {
    /// The filename of the VMDK file (e.g., "TestVM.vmdk").
    pub file_name: String,
    /// The controller type and number (e.g., "scsi0", "ide0", "nvme0", "sata0").
    pub controller: String,
    /// The unit number on the controller (e.g., 0, 1, 2).
    pub unit: u32,
}

/// Configuration for a network adapter attached to the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkConfig {
    /// The network adapter name (e.g., "ethernet0").
    pub name: String,
    /// The virtual device type (e.g., "e1000", "vmxnet3").
    pub virtual_dev: Option<String>,
    /// The network name this adapter is connected to (e.g., "NAT", "Bridged").
    pub network_name: Option<String>,
}

/// Parsed VMX configuration containing VM settings.
#[derive(Debug, Clone)]
pub struct VmxConfig {
    /// The display name of the VM.
    pub display_name: String,
    /// The guest operating system type.
    pub guest_os: String,
    /// Memory size in megabytes.
    pub memory_mb: u32,
    /// Number of virtual CPUs.
    pub num_cpus: u32,
    /// List of attached disk configurations.
    pub disks: Vec<DiskConfig>,
    /// List of network adapter configurations.
    pub networks: Vec<NetworkConfig>,
    /// Raw key-value pairs from the VMX file.
    pub raw: HashMap<String, String>,
}

/// Parse a VMX file and extract VM configuration.
///
/// # Arguments
///
/// * `path` - Path to the VMX file to parse.
///
/// # Returns
///
/// A `VmxConfig` containing the parsed configuration.
///
/// # Errors
///
/// Returns an error if the file cannot be read or if required fields are missing.
pub fn parse_vmx(path: &Path) -> Result<VmxConfig> {
    let content = fs::read_to_string(path).map_err(|e| Error::io(e, path))?;
    parse_vmx_content(&content)
}

/// Parse VMX content from a string.
///
/// This is useful for testing without file I/O.
fn parse_vmx_content(content: &str) -> Result<VmxConfig> {
    let raw = parse_key_value_pairs(content);

    let display_name = raw
        .get("displayName")
        .cloned()
        .unwrap_or_else(|| "Unnamed VM".to_string());

    let guest_os = raw
        .get("guestOS")
        .cloned()
        .unwrap_or_else(|| "other".to_string());

    let memory_mb = raw
        .get("memsize")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1024);

    let num_cpus = raw
        .get("numvcpus")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1);

    let disks = extract_disks(&raw);
    let networks = extract_networks(&raw);

    Ok(VmxConfig {
        display_name,
        guest_os,
        memory_mb,
        num_cpus,
        disks,
        networks,
        raw,
    })
}

/// Parse key-value pairs from VMX content.
///
/// Handles both quoted and unquoted values:
/// - `key = "value"` -> ("key", "value")
/// - `key = value` -> ("key", "value")
fn parse_key_value_pairs(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Find the first '=' to split key and value
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim();
            let value = line[eq_pos + 1..].trim();

            // Remove surrounding quotes if present
            let value = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                &value[1..value.len() - 1]
            } else {
                value
            };

            map.insert(key.to_string(), value.to_string());
        }
    }

    map
}

/// Extract disk configurations from the raw key-value pairs.
///
/// Looks for patterns like:
/// - scsi0:0.fileName = "disk.vmdk"
/// - ide0:0.fileName = "disk.vmdk"
/// - nvme0:0.fileName = "disk.vmdk"
/// - sata0:0.fileName = "disk.vmdk"
fn extract_disks(raw: &HashMap<String, String>) -> Vec<DiskConfig> {
    let mut disks = Vec::new();
    let controller_prefixes = ["scsi", "ide", "nvme", "sata"];

    for (key, value) in raw {
        // Check if this is a fileName entry
        if !key.ends_with(".fileName") {
            continue;
        }

        // Skip non-VMDK files (like .iso files)
        if !value.ends_with(".vmdk") {
            continue;
        }

        // Parse the controller:unit prefix (e.g., "scsi0:0")
        let prefix = &key[..key.len() - ".fileName".len()];

        // Check if it starts with a known controller type
        let mut matched = false;
        for ctrl_prefix in &controller_prefixes {
            if prefix.starts_with(*ctrl_prefix) {
                matched = true;
                break;
            }
        }

        if !matched {
            continue;
        }

        // Parse controller and unit from "scsi0:0" format
        if let Some(colon_pos) = prefix.find(':') {
            let controller = &prefix[..colon_pos];
            let unit_str = &prefix[colon_pos + 1..];

            if let Ok(unit) = unit_str.parse::<u32>() {
                // Check if this disk is present
                let present_key = format!("{}.present", prefix);
                let is_present = raw
                    .get(&present_key)
                    .map(|v| v.eq_ignore_ascii_case("TRUE"))
                    .unwrap_or(false);

                if is_present {
                    disks.push(DiskConfig {
                        file_name: value.clone(),
                        controller: controller.to_string(),
                        unit,
                    });
                }
            }
        }
    }

    // Sort disks by controller and unit for consistent ordering
    disks.sort_by(|a, b| {
        a.controller
            .cmp(&b.controller)
            .then_with(|| a.unit.cmp(&b.unit))
    });

    disks
}

/// Extract network configurations from the raw key-value pairs.
///
/// Looks for patterns like:
/// - ethernet0.present = "TRUE"
/// - ethernet0.virtualDev = "e1000"
/// - ethernet0.networkName = "NAT"
fn extract_networks(raw: &HashMap<String, String>) -> Vec<NetworkConfig> {
    let mut networks = Vec::new();
    let mut network_names: Vec<String> = Vec::new();

    // First, find all present network adapters
    for (key, value) in raw {
        if key.starts_with("ethernet")
            && key.ends_with(".present")
            && value.eq_ignore_ascii_case("TRUE")
        {
            let name = &key[..key.len() - ".present".len()];
            network_names.push(name.to_string());
        }
    }

    // Sort for consistent ordering
    network_names.sort();

    // Then, extract details for each network adapter
    for name in network_names {
        let virtual_dev_key = format!("{}.virtualDev", name);
        let network_name_key = format!("{}.networkName", name);

        let virtual_dev = raw.get(&virtual_dev_key).cloned();
        let network_name = raw.get(&network_name_key).cloned();

        networks.push(NetworkConfig {
            name,
            virtual_dev,
            network_name,
        });
    }

    networks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_value_pairs_quoted() {
        let content = r#"
            displayName = "My VM"
            memsize = "2048"
        "#;
        let map = parse_key_value_pairs(content);
        assert_eq!(map.get("displayName"), Some(&"My VM".to_string()));
        assert_eq!(map.get("memsize"), Some(&"2048".to_string()));
    }

    #[test]
    fn test_parse_key_value_pairs_unquoted() {
        let content = r#"
            displayName = MyVM
            memsize = 2048
        "#;
        let map = parse_key_value_pairs(content);
        assert_eq!(map.get("displayName"), Some(&"MyVM".to_string()));
        assert_eq!(map.get("memsize"), Some(&"2048".to_string()));
    }

    #[test]
    fn test_parse_key_value_pairs_skips_comments() {
        let content = r#"
            # This is a comment
            displayName = "Test"
        "#;
        let map = parse_key_value_pairs(content);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("displayName"), Some(&"Test".to_string()));
    }

    #[test]
    fn test_parse_key_value_pairs_skips_empty_lines() {
        let content = r#"
            displayName = "Test"

            memsize = "1024"
        "#;
        let map = parse_key_value_pairs(content);
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_extract_disks_scsi() {
        let mut raw = HashMap::new();
        raw.insert("scsi0:0.present".to_string(), "TRUE".to_string());
        raw.insert("scsi0:0.fileName".to_string(), "disk.vmdk".to_string());

        let disks = extract_disks(&raw);
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].file_name, "disk.vmdk");
        assert_eq!(disks[0].controller, "scsi0");
        assert_eq!(disks[0].unit, 0);
    }

    #[test]
    fn test_extract_disks_multiple_controllers() {
        let mut raw = HashMap::new();
        raw.insert("scsi0:0.present".to_string(), "TRUE".to_string());
        raw.insert("scsi0:0.fileName".to_string(), "disk1.vmdk".to_string());
        raw.insert("nvme0:0.present".to_string(), "TRUE".to_string());
        raw.insert("nvme0:0.fileName".to_string(), "disk2.vmdk".to_string());

        let disks = extract_disks(&raw);
        assert_eq!(disks.len(), 2);
    }

    #[test]
    fn test_extract_disks_skips_not_present() {
        let mut raw = HashMap::new();
        raw.insert("scsi0:0.present".to_string(), "FALSE".to_string());
        raw.insert("scsi0:0.fileName".to_string(), "disk.vmdk".to_string());

        let disks = extract_disks(&raw);
        assert_eq!(disks.len(), 0);
    }

    #[test]
    fn test_extract_disks_skips_iso_files() {
        let mut raw = HashMap::new();
        raw.insert("ide0:0.present".to_string(), "TRUE".to_string());
        raw.insert("ide0:0.fileName".to_string(), "ubuntu.iso".to_string());

        let disks = extract_disks(&raw);
        assert_eq!(disks.len(), 0);
    }

    #[test]
    fn test_extract_networks() {
        let mut raw = HashMap::new();
        raw.insert("ethernet0.present".to_string(), "TRUE".to_string());
        raw.insert("ethernet0.virtualDev".to_string(), "vmxnet3".to_string());
        raw.insert("ethernet0.networkName".to_string(), "Bridged".to_string());

        let networks = extract_networks(&raw);
        assert_eq!(networks.len(), 1);
        assert_eq!(networks[0].name, "ethernet0");
        assert_eq!(networks[0].virtual_dev, Some("vmxnet3".to_string()));
        assert_eq!(networks[0].network_name, Some("Bridged".to_string()));
    }

    #[test]
    fn test_extract_networks_optional_fields() {
        let mut raw = HashMap::new();
        raw.insert("ethernet0.present".to_string(), "TRUE".to_string());

        let networks = extract_networks(&raw);
        assert_eq!(networks.len(), 1);
        assert_eq!(networks[0].name, "ethernet0");
        assert_eq!(networks[0].virtual_dev, None);
        assert_eq!(networks[0].network_name, None);
    }

    #[test]
    fn test_parse_vmx_content_defaults() {
        let content = "";
        let config = parse_vmx_content(content).unwrap();

        assert_eq!(config.display_name, "Unnamed VM");
        assert_eq!(config.guest_os, "other");
        assert_eq!(config.memory_mb, 1024);
        assert_eq!(config.num_cpus, 1);
        assert_eq!(config.disks.len(), 0);
        assert_eq!(config.networks.len(), 0);
    }

    #[test]
    fn test_parse_vmx_content_full() {
        let content = r#"
            displayName = "TestVM"
            guestOS = "ubuntu-64"
            memsize = "4096"
            numvcpus = "2"
            scsi0:0.present = "TRUE"
            scsi0:0.fileName = "disk.vmdk"
            ethernet0.present = "TRUE"
        "#;
        let config = parse_vmx_content(content).unwrap();

        assert_eq!(config.display_name, "TestVM");
        assert_eq!(config.guest_os, "ubuntu-64");
        assert_eq!(config.memory_mb, 4096);
        assert_eq!(config.num_cpus, 2);
        assert_eq!(config.disks.len(), 1);
        assert_eq!(config.networks.len(), 1);
    }
}
