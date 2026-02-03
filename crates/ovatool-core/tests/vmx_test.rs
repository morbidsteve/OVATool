//! Integration tests for VMX parsing.

use ovatool_core::vmx::parse_vmx;
use std::path::Path;

fn fixture_path() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/test.vmx"
    ))
}

#[test]
fn test_parse_vmx_display_name() {
    let config = parse_vmx(fixture_path()).expect("Failed to parse VMX");
    assert_eq!(config.display_name, "TestVM");
}

#[test]
fn test_parse_vmx_memory() {
    let config = parse_vmx(fixture_path()).expect("Failed to parse VMX");
    assert_eq!(config.memory_mb, 4096);
}

#[test]
fn test_parse_vmx_cpus() {
    let config = parse_vmx(fixture_path()).expect("Failed to parse VMX");
    assert_eq!(config.num_cpus, 2);
}

#[test]
fn test_parse_vmx_disks() {
    let config = parse_vmx(fixture_path()).expect("Failed to parse VMX");
    assert_eq!(config.disks.len(), 1);

    let disk = &config.disks[0];
    assert_eq!(disk.file_name, "TestVM.vmdk");
    assert_eq!(disk.controller, "scsi0");
    assert_eq!(disk.unit, 0);
}

#[test]
fn test_parse_vmx_guest_os() {
    let config = parse_vmx(fixture_path()).expect("Failed to parse VMX");
    assert_eq!(config.guest_os, "ubuntu-64");
}

#[test]
fn test_parse_vmx_networks() {
    let config = parse_vmx(fixture_path()).expect("Failed to parse VMX");
    assert_eq!(config.networks.len(), 1);

    let network = &config.networks[0];
    assert_eq!(network.name, "ethernet0");
    assert_eq!(network.virtual_dev, Some("e1000".to_string()));
    assert_eq!(network.network_name, Some("NAT".to_string()));
}

#[test]
fn test_parse_vmx_raw_values() {
    let config = parse_vmx(fixture_path()).expect("Failed to parse VMX");

    // Verify raw HashMap contains all key-value pairs
    assert_eq!(config.raw.get("config.version"), Some(&"8".to_string()));
    assert_eq!(config.raw.get("virtualHW.version"), Some(&"21".to_string()));
    assert_eq!(config.raw.get(".encoding"), Some(&"UTF-8".to_string()));
}
