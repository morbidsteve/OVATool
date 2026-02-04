//! Integration tests for VMDK descriptor parsing.

use ovatool_core::vmdk::descriptor::{parse_descriptor, ExtentType};

const MONOLITHIC_FLAT_DESCRIPTOR: &str = r#"
# Disk DescriptorFile
version=1
CID=fffffffe
parentCID=ffffffff
createType="monolithicFlat"

# Extent description
RW 838860800 FLAT "TestVM-flat.vmdk" 0

# The Disk Data Base
ddb.virtualHWVersion = "21"
ddb.geometry.cylinders = "52216"
ddb.geometry.heads = "16"
ddb.geometry.sectors = "63"
ddb.adapterType = "lsilogic"
"#;

#[test]
fn test_parse_create_type() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");
    assert_eq!(descriptor.create_type, "monolithicFlat");
}

#[test]
fn test_parse_extent() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");

    assert_eq!(descriptor.extents.len(), 1);

    let extent = &descriptor.extents[0];
    assert_eq!(extent.access, "RW");
    assert_eq!(extent.size_sectors, 838860800);
    assert_eq!(extent.extent_type, ExtentType::Flat);
    assert_eq!(extent.filename, "TestVM-flat.vmdk");
    assert_eq!(extent.offset, 0);
}

#[test]
fn test_parse_geometry() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");

    assert_eq!(descriptor.cylinders, 52216);
    assert_eq!(descriptor.heads, 16);
    assert_eq!(descriptor.sectors, 63);
}

#[test]
fn test_disk_size_bytes() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");

    // 838860800 sectors * 512 bytes per sector
    assert_eq!(descriptor.disk_size_bytes(), 838860800_u64 * 512);
}

#[test]
fn test_disk_size_sectors() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");

    assert_eq!(descriptor.disk_size_sectors(), 838860800);
}

#[test]
fn test_parse_version() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");
    assert_eq!(descriptor.version, 1);
}

#[test]
fn test_parse_cid() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");
    assert_eq!(descriptor.cid, 0xfffffffe);
}

#[test]
fn test_parse_parent_cid() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");
    assert_eq!(descriptor.parent_cid, 0xffffffff);
}

#[test]
fn test_parse_adapter_type() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");
    assert_eq!(descriptor.adapter_type, "lsilogic");
}

#[test]
fn test_parse_hw_version() {
    let descriptor =
        parse_descriptor(MONOLITHIC_FLAT_DESCRIPTOR).expect("Failed to parse descriptor");
    assert_eq!(descriptor.hw_version, "21");
}
