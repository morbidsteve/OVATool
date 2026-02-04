//! Integration tests for VMDK descriptor parsing.

use ovatool_core::vmdk::descriptor::{parse_descriptor, ExtentType};

const SPLIT_SPARSE_DESCRIPTOR: &str = r#"
# Disk DescriptorFile
version=1
CID=12345678
parentCID=ffffffff
createType="twoGbMaxExtentSparse"

# Extent description
RW 4194304 SPARSE "Ubuntu 64-bit-s001.vmdk"
RW 4194304 SPARSE "Ubuntu 64-bit-s002.vmdk"
RW 4194304 SPARSE "Ubuntu 64-bit-s003.vmdk"
RW 2097152 SPARSE "Ubuntu 64-bit-s004.vmdk"

# The Disk Data Base
ddb.virtualHWVersion = "21"
ddb.geometry.cylinders = "913"
ddb.geometry.heads = "16"
ddb.geometry.sectors = "63"
ddb.adapterType = "lsilogic"
"#;

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

// Tests for split sparse (twoGbMaxExtentSparse) VMDK descriptors

#[test]
fn test_parse_split_sparse_create_type() {
    let descriptor =
        parse_descriptor(SPLIT_SPARSE_DESCRIPTOR).expect("Failed to parse descriptor");
    assert_eq!(descriptor.create_type, "twoGbMaxExtentSparse");
}

#[test]
fn test_parse_split_sparse_extents() {
    let descriptor =
        parse_descriptor(SPLIT_SPARSE_DESCRIPTOR).expect("Failed to parse descriptor");

    assert_eq!(descriptor.extents.len(), 4);

    // First extent
    assert_eq!(descriptor.extents[0].access, "RW");
    assert_eq!(descriptor.extents[0].size_sectors, 4194304);
    assert_eq!(descriptor.extents[0].extent_type, ExtentType::Sparse);
    assert_eq!(descriptor.extents[0].filename, "Ubuntu 64-bit-s001.vmdk");
    assert_eq!(descriptor.extents[0].offset, 0);

    // Second extent
    assert_eq!(descriptor.extents[1].extent_type, ExtentType::Sparse);
    assert_eq!(descriptor.extents[1].filename, "Ubuntu 64-bit-s002.vmdk");

    // Last extent (smaller)
    assert_eq!(descriptor.extents[3].size_sectors, 2097152);
    assert_eq!(descriptor.extents[3].filename, "Ubuntu 64-bit-s004.vmdk");
}

#[test]
fn test_split_sparse_total_size() {
    let descriptor =
        parse_descriptor(SPLIT_SPARSE_DESCRIPTOR).expect("Failed to parse descriptor");

    // 3 * 4194304 + 2097152 = 14680064 sectors
    let expected_sectors = 4194304u64 * 3 + 2097152;
    assert_eq!(descriptor.disk_size_sectors(), expected_sectors);

    // In bytes
    assert_eq!(descriptor.disk_size_bytes(), expected_sectors * 512);
}

#[test]
fn test_split_sparse_has_no_flat_extents() {
    let descriptor =
        parse_descriptor(SPLIT_SPARSE_DESCRIPTOR).expect("Failed to parse descriptor");

    let flat_extents: Vec<_> = descriptor
        .extents
        .iter()
        .filter(|e| e.extent_type == ExtentType::Flat)
        .collect();

    assert!(flat_extents.is_empty(), "Split sparse should have no flat extents");
}

#[test]
fn test_split_sparse_all_extents_are_sparse() {
    let descriptor =
        parse_descriptor(SPLIT_SPARSE_DESCRIPTOR).expect("Failed to parse descriptor");

    for extent in &descriptor.extents {
        assert_eq!(extent.extent_type, ExtentType::Sparse);
    }
}
