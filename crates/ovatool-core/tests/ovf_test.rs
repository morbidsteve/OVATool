//! Integration tests for OVF XML builder.

use ovatool_core::ovf::{DiskInfo, OvfBuilder};
use ovatool_core::vmx::{DiskConfig, NetworkConfig, VmxConfig};
use std::collections::HashMap;

/// Create a test VMX configuration for use in tests.
fn create_test_config() -> VmxConfig {
    VmxConfig {
        display_name: "TestVM".to_string(),
        guest_os: "ubuntu-64".to_string(),
        memory_mb: 4096,
        num_cpus: 2,
        disks: vec![DiskConfig {
            file_name: "TestVM.vmdk".to_string(),
            controller: "scsi0".to_string(),
            unit: 0,
        }],
        networks: vec![NetworkConfig {
            name: "ethernet0".to_string(),
            virtual_dev: Some("e1000".to_string()),
            network_name: Some("NAT".to_string()),
        }],
        raw: HashMap::new(),
    }
}

/// Create test disk info for use in tests.
fn create_test_disks() -> Vec<DiskInfo> {
    vec![DiskInfo {
        id: "vmdisk1".to_string(),
        file_ref: "file1".to_string(),
        capacity_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
        file_size_bytes: 100 * 1024 * 1024,      // 100 MB
    }]
}

#[test]
fn test_ovf_envelope() {
    let config = create_test_config();
    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify contains "ovf:Envelope" and "xmlns:ovf="
    assert!(
        ovf.contains("ovf:Envelope"),
        "OVF should contain 'ovf:Envelope'"
    );
    assert!(
        ovf.contains("xmlns:ovf="),
        "OVF should contain 'xmlns:ovf='"
    );
    assert!(
        ovf.contains("http://schemas.dmtf.org/ovf/envelope/1"),
        "OVF should contain the OVF namespace URL"
    );

    // Verify all required namespaces are present
    assert!(
        ovf.contains("xmlns:rasd="),
        "OVF should contain RASD namespace"
    );
    assert!(
        ovf.contains("xmlns:vssd="),
        "OVF should contain VSSD namespace"
    );
    assert!(
        ovf.contains("xmlns:vmw="),
        "OVF should contain VMware namespace"
    );
    assert!(
        ovf.contains("xmlns:xsi="),
        "OVF should contain XSI namespace"
    );

    // Verify closing tag
    assert!(
        ovf.contains("</ovf:Envelope>"),
        "OVF should have closing Envelope tag"
    );
}

#[test]
fn test_ovf_virtual_system() {
    let config = create_test_config();
    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify contains "VirtualSystem" and VM name
    assert!(
        ovf.contains("VirtualSystem"),
        "OVF should contain 'VirtualSystem'"
    );
    assert!(
        ovf.contains("ovf:id=\"TestVM\""),
        "OVF should contain VM ID 'TestVM'"
    );
    assert!(
        ovf.contains("<ovf:Name>TestVM</ovf:Name>"),
        "OVF should contain VM name 'TestVM'"
    );

    // Verify OperatingSystemSection
    assert!(
        ovf.contains("OperatingSystemSection"),
        "OVF should contain OperatingSystemSection"
    );
    assert!(
        ovf.contains("ubuntu64Guest"),
        "OVF should contain mapped guest OS type"
    );
}

#[test]
fn test_ovf_hardware_section() {
    let config = create_test_config();
    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify contains "VirtualHardwareSection", memory value, CPU count
    assert!(
        ovf.contains("VirtualHardwareSection"),
        "OVF should contain 'VirtualHardwareSection'"
    );

    // Memory (4096 MB)
    assert!(
        ovf.contains("<rasd:VirtualQuantity>4096</rasd:VirtualQuantity>"),
        "OVF should contain memory value 4096"
    );

    // CPU count (2)
    assert!(
        ovf.contains("<rasd:VirtualQuantity>2</rasd:VirtualQuantity>"),
        "OVF should contain CPU count 2"
    );

    // Resource types
    assert!(
        ovf.contains("<rasd:ResourceType>3</rasd:ResourceType>"),
        "OVF should contain CPU ResourceType 3"
    );
    assert!(
        ovf.contains("<rasd:ResourceType>4</rasd:ResourceType>"),
        "OVF should contain Memory ResourceType 4"
    );
    assert!(
        ovf.contains("<rasd:ResourceType>6</rasd:ResourceType>"),
        "OVF should contain SCSI Controller ResourceType 6"
    );
    assert!(
        ovf.contains("<rasd:ResourceType>10</rasd:ResourceType>"),
        "OVF should contain Network ResourceType 10"
    );
    assert!(
        ovf.contains("<rasd:ResourceType>17</rasd:ResourceType>"),
        "OVF should contain Disk ResourceType 17"
    );
}

#[test]
fn test_ovf_disk_section() {
    let config = create_test_config();
    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify contains "DiskSection" and disk ID
    assert!(
        ovf.contains("DiskSection"),
        "OVF should contain 'DiskSection'"
    );
    assert!(
        ovf.contains("ovf:diskId=\"vmdisk1\""),
        "OVF should contain disk ID 'vmdisk1'"
    );

    // Verify disk capacity
    let capacity = 10 * 1024 * 1024 * 1024u64;
    assert!(
        ovf.contains(&format!("ovf:capacity=\"{}\"", capacity)),
        "OVF should contain disk capacity"
    );

    // Verify disk format
    assert!(
        ovf.contains("vmdk.html#streamOptimized"),
        "OVF should contain streamOptimized format"
    );

    // Verify file reference
    assert!(
        ovf.contains("ovf:fileRef=\"file1\""),
        "OVF should contain file reference"
    );
}

#[test]
fn test_ovf_references_section() {
    let config = create_test_config();
    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify References section
    assert!(
        ovf.contains("ovf:References"),
        "OVF should contain References section"
    );
    assert!(
        ovf.contains("ovf:File"),
        "OVF should contain File element"
    );
    assert!(
        ovf.contains("ovf:href=\"TestVM.vmdk\""),
        "OVF should contain file href"
    );
    assert!(
        ovf.contains("ovf:id=\"file1\""),
        "OVF should contain file id"
    );

    // Verify file size
    let file_size = 100 * 1024 * 1024u64;
    assert!(
        ovf.contains(&format!("ovf:size=\"{}\"", file_size)),
        "OVF should contain file size"
    );
}

#[test]
fn test_ovf_network_section() {
    let config = create_test_config();
    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify NetworkSection
    assert!(
        ovf.contains("NetworkSection"),
        "OVF should contain NetworkSection"
    );
    assert!(
        ovf.contains("ovf:name=\"NAT\""),
        "OVF should contain network name 'NAT'"
    );

    // Verify network adapter in hardware section
    assert!(
        ovf.contains("<rasd:Connection>NAT</rasd:Connection>"),
        "OVF should contain network connection"
    );
    assert!(
        ovf.contains("e1000"),
        "OVF should contain network adapter type"
    );
}

#[test]
fn test_ovf_xml_declaration() {
    let config = create_test_config();
    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify XML declaration
    assert!(
        ovf.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        "OVF should start with XML declaration"
    );
}

#[test]
fn test_ovf_multiple_disks() {
    let mut config = create_test_config();
    config.disks.push(DiskConfig {
        file_name: "TestVM_1.vmdk".to_string(),
        controller: "scsi0".to_string(),
        unit: 1,
    });

    let builder = OvfBuilder::new(&config);
    let disks = vec![
        DiskInfo {
            id: "vmdisk1".to_string(),
            file_ref: "file1".to_string(),
            capacity_bytes: 10 * 1024 * 1024 * 1024,
            file_size_bytes: 100 * 1024 * 1024,
        },
        DiskInfo {
            id: "vmdisk2".to_string(),
            file_ref: "file2".to_string(),
            capacity_bytes: 20 * 1024 * 1024 * 1024,
            file_size_bytes: 200 * 1024 * 1024,
        },
    ];

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify both disks are present
    assert!(
        ovf.contains("ovf:diskId=\"vmdisk1\""),
        "OVF should contain first disk ID"
    );
    assert!(
        ovf.contains("ovf:diskId=\"vmdisk2\""),
        "OVF should contain second disk ID"
    );
    assert!(
        ovf.contains("ovf:href=\"TestVM.vmdk\""),
        "OVF should contain first disk filename"
    );
    assert!(
        ovf.contains("ovf:href=\"TestVM_1.vmdk\""),
        "OVF should contain second disk filename"
    );
}

#[test]
fn test_ovf_windows_guest_os() {
    let mut config = create_test_config();
    config.guest_os = "windows10-64".to_string();

    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify Windows guest OS mapping
    assert!(
        ovf.contains("windows9_64Guest"),
        "OVF should contain Windows guest OS type"
    );
}

#[test]
fn test_ovf_no_networks() {
    let mut config = create_test_config();
    config.networks.clear();

    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Should have default network
    assert!(
        ovf.contains("ovf:name=\"VM Network\""),
        "OVF should contain default network name"
    );
}

#[test]
fn test_ovf_special_characters_escaped() {
    let mut config = create_test_config();
    config.display_name = "Test<VM>&\"Name'".to_string();

    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify XML special characters are escaped in Name element
    assert!(
        ovf.contains("&lt;"),
        "OVF should escape < character"
    );
    assert!(
        ovf.contains("&gt;"),
        "OVF should escape > character"
    );
    assert!(
        ovf.contains("&amp;"),
        "OVF should escape & character"
    );
}

#[test]
fn test_ovf_scsi_controller() {
    let config = create_test_config();
    let builder = OvfBuilder::new(&config);
    let disks = create_test_disks();

    let ovf = builder.build(&disks).expect("Failed to build OVF");

    // Verify SCSI controller
    assert!(
        ovf.contains("SCSI Controller"),
        "OVF should contain SCSI controller"
    );
    assert!(
        ovf.contains("lsilogic"),
        "OVF should contain lsilogic controller type"
    );
}
