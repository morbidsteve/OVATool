//! OVF descriptor generation.
//!
//! This module generates OVF (Open Virtualization Format) XML descriptors
//! from VMX metadata. The generated OVF is compatible with VMware and other
//! virtualization platforms that support the OVF 1.0 specification.

use crate::error::Result;
use crate::vmx::VmxConfig;

/// Information about a disk to include in the OVF.
#[derive(Debug, Clone)]
pub struct DiskInfo {
    /// Unique identifier for this disk (e.g., "vmdisk1").
    pub id: String,
    /// Reference to the file in the References section (e.g., "file1").
    pub file_ref: String,
    /// The capacity of the disk in bytes.
    pub capacity_bytes: u64,
    /// The actual file size of the disk in bytes.
    pub file_size_bytes: u64,
}

/// Builder for generating OVF XML descriptors.
pub struct OvfBuilder<'a> {
    config: &'a VmxConfig,
}

impl<'a> OvfBuilder<'a> {
    /// Create a new OVF builder from a VMX configuration.
    pub fn new(config: &'a VmxConfig) -> Self {
        Self { config }
    }

    /// Build the OVF XML descriptor.
    ///
    /// # Arguments
    ///
    /// * `disks` - Information about the disks to include in the OVF.
    ///
    /// # Returns
    ///
    /// A string containing the complete OVF XML document.
    pub fn build(&self, disks: &[DiskInfo]) -> Result<String> {
        let mut xml = String::new();

        // XML declaration
        xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        xml.push('\n');

        // Envelope opening with namespaces
        xml.push_str(&self.build_envelope_open());

        // References section
        xml.push_str(&self.build_references(disks));

        // DiskSection
        xml.push_str(&self.build_disk_section(disks));

        // NetworkSection
        xml.push_str(&self.build_network_section());

        // VirtualSystem
        xml.push_str(&self.build_virtual_system(disks));

        // Envelope closing
        xml.push_str("</ovf:Envelope>\n");

        Ok(xml)
    }

    /// Build the opening Envelope tag with all required namespaces.
    fn build_envelope_open(&self) -> String {
        r#"<ovf:Envelope xmlns:ovf="http://schemas.dmtf.org/ovf/envelope/1"
    xmlns:rasd="http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/CIM_ResourceAllocationSettingData"
    xmlns:vssd="http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/CIM_VirtualSystemSettingData"
    xmlns:vmw="http://www.vmware.com/schema/ovf"
    xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
"#
        .to_string()
    }

    /// Build the References section listing all disk files.
    fn build_references(&self, disks: &[DiskInfo]) -> String {
        let mut xml = String::new();
        xml.push_str("  <ovf:References>\n");

        for (i, disk) in disks.iter().enumerate() {
            let filename = if i < self.config.disks.len() {
                &self.config.disks[i].file_name
            } else {
                "disk.vmdk"
            };
            xml.push_str(&format!(
                "    <ovf:File ovf:href=\"{}\" ovf:id=\"{}\" ovf:size=\"{}\"/>\n",
                filename, disk.file_ref, disk.file_size_bytes
            ));
        }

        xml.push_str("  </ovf:References>\n");
        xml
    }

    /// Build the DiskSection describing disk capacities and formats.
    fn build_disk_section(&self, disks: &[DiskInfo]) -> String {
        let mut xml = String::new();
        xml.push_str("  <ovf:DiskSection>\n");
        xml.push_str("    <ovf:Info>Virtual disk information</ovf:Info>\n");

        for disk in disks {
            xml.push_str(&format!(
                "    <ovf:Disk ovf:capacity=\"{}\" ovf:capacityAllocationUnits=\"byte\" ovf:diskId=\"{}\" ovf:fileRef=\"{}\" ovf:format=\"http://www.vmware.com/interfaces/specifications/vmdk.html#streamOptimized\"/>\n",
                disk.capacity_bytes, escape_xml(&disk.id), disk.file_ref
            ));
        }

        xml.push_str("  </ovf:DiskSection>\n");
        xml
    }

    /// Build the NetworkSection describing network connections.
    fn build_network_section(&self) -> String {
        let mut xml = String::new();
        xml.push_str("  <ovf:NetworkSection>\n");
        xml.push_str("    <ovf:Info>Network configuration</ovf:Info>\n");

        if self.config.networks.is_empty() {
            // Default network if none specified
            xml.push_str("    <ovf:Network ovf:name=\"VM Network\">\n");
            xml.push_str("      <ovf:Description>The VM Network</ovf:Description>\n");
            xml.push_str("    </ovf:Network>\n");
        } else {
            for network in &self.config.networks {
                let network_name = network
                    .network_name
                    .as_deref()
                    .unwrap_or("VM Network");
                xml.push_str(&format!(
                    "    <ovf:Network ovf:name=\"{}\">\n",
                    escape_xml(network_name)
                ));
                xml.push_str(&format!(
                    "      <ovf:Description>The {} network</ovf:Description>\n",
                    escape_xml(network_name)
                ));
                xml.push_str("    </ovf:Network>\n");
            }
        }

        xml.push_str("  </ovf:NetworkSection>\n");
        xml
    }

    /// Build the VirtualSystem section with hardware configuration.
    fn build_virtual_system(&self, disks: &[DiskInfo]) -> String {
        let mut xml = String::new();
        let vm_id = sanitize_id(&self.config.display_name);

        xml.push_str(&format!(
            "  <ovf:VirtualSystem ovf:id=\"{}\">\n",
            escape_xml(&vm_id)
        ));
        xml.push_str("    <ovf:Info>A virtual machine</ovf:Info>\n");
        xml.push_str(&format!(
            "    <ovf:Name>{}</ovf:Name>\n",
            escape_xml(&self.config.display_name)
        ));

        // Operating System Section
        xml.push_str(&self.build_os_section());

        // Virtual Hardware Section
        xml.push_str(&self.build_hardware_section(disks));

        xml.push_str("  </ovf:VirtualSystem>\n");
        xml
    }

    /// Build the OperatingSystemSection.
    fn build_os_section(&self) -> String {
        let (os_id, os_type) = map_guest_os(&self.config.guest_os);

        let mut xml = String::new();
        xml.push_str(&format!(
            "    <ovf:OperatingSystemSection ovf:id=\"{}\" vmw:osType=\"{}\">\n",
            os_id, os_type
        ));
        xml.push_str("      <ovf:Info>The guest operating system</ovf:Info>\n");
        xml.push_str(&format!(
            "      <ovf:Description>{}</ovf:Description>\n",
            escape_xml(&self.config.guest_os)
        ));
        xml.push_str("    </ovf:OperatingSystemSection>\n");
        xml
    }

    /// Build the VirtualHardwareSection with CPU, memory, controllers, disks, and networks.
    fn build_hardware_section(&self, disks: &[DiskInfo]) -> String {
        let mut xml = String::new();
        xml.push_str("    <ovf:VirtualHardwareSection>\n");
        xml.push_str("      <ovf:Info>Virtual hardware requirements</ovf:Info>\n");

        // System info
        xml.push_str(&self.build_system_item());

        // CPU item (ResourceType=3)
        xml.push_str(&self.build_cpu_item());

        // Memory item (ResourceType=4)
        xml.push_str(&self.build_memory_item());

        // SCSI Controller (ResourceType=6)
        xml.push_str(&self.build_scsi_controller());

        // Disk items (ResourceType=17)
        for (i, disk) in disks.iter().enumerate() {
            xml.push_str(&self.build_disk_item(i, disk));
        }

        // Network adapters (ResourceType=10)
        for (i, _network) in self.config.networks.iter().enumerate() {
            xml.push_str(&self.build_network_item(i));
        }

        // If no networks defined, add a default one
        if self.config.networks.is_empty() {
            xml.push_str(&self.build_default_network_item());
        }

        xml.push_str("    </ovf:VirtualHardwareSection>\n");
        xml
    }

    /// Build the System item describing the virtual system type.
    fn build_system_item(&self) -> String {
        let mut xml = String::new();
        xml.push_str("      <ovf:System>\n");
        xml.push_str("        <vssd:ElementName>Virtual Hardware Family</vssd:ElementName>\n");
        xml.push_str("        <vssd:InstanceID>0</vssd:InstanceID>\n");
        xml.push_str(&format!(
            "        <vssd:VirtualSystemIdentifier>{}</vssd:VirtualSystemIdentifier>\n",
            escape_xml(&self.config.display_name)
        ));
        xml.push_str("        <vssd:VirtualSystemType>vmx-21</vssd:VirtualSystemType>\n");
        xml.push_str("      </ovf:System>\n");
        xml
    }

    /// Build the CPU hardware item.
    fn build_cpu_item(&self) -> String {
        let mut xml = String::new();
        xml.push_str("      <ovf:Item>\n");
        xml.push_str("        <rasd:AllocationUnits>hertz * 10^6</rasd:AllocationUnits>\n");
        xml.push_str("        <rasd:Description>Number of Virtual CPUs</rasd:Description>\n");
        xml.push_str("        <rasd:ElementName>CPU</rasd:ElementName>\n");
        xml.push_str("        <rasd:InstanceID>1</rasd:InstanceID>\n");
        xml.push_str("        <rasd:ResourceType>3</rasd:ResourceType>\n");
        xml.push_str(&format!(
            "        <rasd:VirtualQuantity>{}</rasd:VirtualQuantity>\n",
            self.config.num_cpus
        ));
        xml.push_str("      </ovf:Item>\n");
        xml
    }

    /// Build the Memory hardware item.
    fn build_memory_item(&self) -> String {
        let mut xml = String::new();
        xml.push_str("      <ovf:Item>\n");
        xml.push_str("        <rasd:AllocationUnits>byte * 2^20</rasd:AllocationUnits>\n");
        xml.push_str("        <rasd:Description>Memory Size</rasd:Description>\n");
        xml.push_str("        <rasd:ElementName>Memory</rasd:ElementName>\n");
        xml.push_str("        <rasd:InstanceID>2</rasd:InstanceID>\n");
        xml.push_str("        <rasd:ResourceType>4</rasd:ResourceType>\n");
        xml.push_str(&format!(
            "        <rasd:VirtualQuantity>{}</rasd:VirtualQuantity>\n",
            self.config.memory_mb
        ));
        xml.push_str("      </ovf:Item>\n");
        xml
    }

    /// Build the SCSI Controller hardware item.
    fn build_scsi_controller(&self) -> String {
        let mut xml = String::new();
        xml.push_str("      <ovf:Item>\n");
        xml.push_str("        <rasd:Address>0</rasd:Address>\n");
        xml.push_str("        <rasd:Description>SCSI Controller</rasd:Description>\n");
        xml.push_str("        <rasd:ElementName>SCSI Controller 0</rasd:ElementName>\n");
        xml.push_str("        <rasd:InstanceID>3</rasd:InstanceID>\n");
        xml.push_str("        <rasd:ResourceSubType>lsilogic</rasd:ResourceSubType>\n");
        xml.push_str("        <rasd:ResourceType>6</rasd:ResourceType>\n");
        xml.push_str("      </ovf:Item>\n");
        xml
    }

    /// Build a disk hardware item.
    fn build_disk_item(&self, index: usize, disk: &DiskInfo) -> String {
        let instance_id = 4 + index; // Start after System(0), CPU(1), Memory(2), SCSI(3)

        let mut xml = String::new();
        xml.push_str("      <ovf:Item>\n");
        xml.push_str(&format!(
            "        <rasd:AddressOnParent>{}</rasd:AddressOnParent>\n",
            index
        ));
        xml.push_str("        <rasd:Description>Hard Disk</rasd:Description>\n");
        xml.push_str(&format!(
            "        <rasd:ElementName>Hard Disk {}</rasd:ElementName>\n",
            index + 1
        ));
        xml.push_str(&format!(
            "        <rasd:HostResource>ovf:/disk/{}</rasd:HostResource>\n",
            escape_xml(&disk.id)
        ));
        xml.push_str(&format!(
            "        <rasd:InstanceID>{}</rasd:InstanceID>\n",
            instance_id
        ));
        xml.push_str("        <rasd:Parent>3</rasd:Parent>\n"); // Parent is SCSI controller
        xml.push_str("        <rasd:ResourceType>17</rasd:ResourceType>\n");
        xml.push_str("      </ovf:Item>\n");
        xml
    }

    /// Build a network adapter hardware item.
    fn build_network_item(&self, index: usize) -> String {
        let instance_id = 4 + self.config.disks.len() + index;
        let network = &self.config.networks[index];

        let network_name = network
            .network_name
            .as_deref()
            .unwrap_or("VM Network");

        let adapter_type = network
            .virtual_dev
            .as_deref()
            .unwrap_or("E1000");

        let mut xml = String::new();
        xml.push_str("      <ovf:Item>\n");
        xml.push_str("        <rasd:AddressOnParent>0</rasd:AddressOnParent>\n");
        xml.push_str("        <rasd:AutomaticAllocation>true</rasd:AutomaticAllocation>\n");
        xml.push_str(&format!(
            "        <rasd:Connection>{}</rasd:Connection>\n",
            escape_xml(network_name)
        ));
        xml.push_str("        <rasd:Description>Network Adapter</rasd:Description>\n");
        xml.push_str(&format!(
            "        <rasd:ElementName>Network Adapter {}</rasd:ElementName>\n",
            index + 1
        ));
        xml.push_str(&format!(
            "        <rasd:InstanceID>{}</rasd:InstanceID>\n",
            instance_id
        ));
        xml.push_str(&format!(
            "        <rasd:ResourceSubType>{}</rasd:ResourceSubType>\n",
            escape_xml(adapter_type)
        ));
        xml.push_str("        <rasd:ResourceType>10</rasd:ResourceType>\n");
        xml.push_str("      </ovf:Item>\n");
        xml
    }

    /// Build a default network adapter if none are configured.
    fn build_default_network_item(&self) -> String {
        let instance_id = 4 + self.config.disks.len();

        let mut xml = String::new();
        xml.push_str("      <ovf:Item>\n");
        xml.push_str("        <rasd:AddressOnParent>0</rasd:AddressOnParent>\n");
        xml.push_str("        <rasd:AutomaticAllocation>true</rasd:AutomaticAllocation>\n");
        xml.push_str("        <rasd:Connection>VM Network</rasd:Connection>\n");
        xml.push_str("        <rasd:Description>Network Adapter</rasd:Description>\n");
        xml.push_str("        <rasd:ElementName>Network Adapter 1</rasd:ElementName>\n");
        xml.push_str(&format!(
            "        <rasd:InstanceID>{}</rasd:InstanceID>\n",
            instance_id
        ));
        xml.push_str("        <rasd:ResourceSubType>E1000</rasd:ResourceSubType>\n");
        xml.push_str("        <rasd:ResourceType>10</rasd:ResourceType>\n");
        xml.push_str("      </ovf:Item>\n");
        xml
    }
}

/// Map VMware guest OS identifiers to OVF OS IDs and types.
///
/// Returns a tuple of (os_id, os_type) where:
/// - os_id is the numeric OVF OS identifier
/// - os_type is the VMware-specific OS type string
fn map_guest_os(guest_os: &str) -> (u32, &'static str) {
    match guest_os.to_lowercase().as_str() {
        // Ubuntu variants
        "ubuntu-64" | "ubuntu64" => (96, "ubuntu64Guest"),
        "ubuntu" | "ubuntu-32" => (93, "ubuntuGuest"),

        // Debian variants
        "debian-64" | "debian64" | "debian10-64" | "debian11-64" | "debian12-64" => {
            (96, "debian10_64Guest")
        }
        "debian" | "debian-32" | "debian10" | "debian11" | "debian12" => (95, "debian10Guest"),

        // CentOS/RHEL variants
        "centos-64" | "centos64" | "centos7-64" | "centos8-64" | "centos9-64" => {
            (107, "centos64Guest")
        }
        "centos" | "centos-32" | "centos7" | "centos8" | "centos9" => (107, "centosGuest"),
        "rhel-64" | "rhel64" | "rhel7-64" | "rhel8-64" | "rhel9-64" => (80, "rhel7_64Guest"),
        "rhel" | "rhel-32" | "rhel7" | "rhel8" | "rhel9" => (79, "rhel7Guest"),

        // Windows variants
        "windows10-64" | "windows10_64" | "win10-64" => (109, "windows9_64Guest"),
        "windows10" | "windows10-32" | "win10" => (108, "windows9Guest"),
        "windows11-64" | "windows11_64" | "win11-64" | "win11" => (109, "windows9_64Guest"),
        "windows7-64" | "windows7_64" | "win7-64" => (105, "windows7_64Guest"),
        "windows7" | "windows7-32" | "win7" => (104, "windows7Guest"),
        "windows8-64" | "windows8_64" | "win8-64" => (107, "windows8_64Guest"),
        "windows8" | "windows8-32" | "win8" => (106, "windows8Guest"),
        "windowsserver2016-64" | "windows2016-64" | "win2016-64" => (112, "windows9Server64Guest"),
        "windowsserver2019-64" | "windows2019-64" | "win2019-64" => (112, "windows9Server64Guest"),
        "windowsserver2022-64" | "windows2022-64" | "win2022-64" => (112, "windows9Server64Guest"),

        // FreeBSD variants
        "freebsd-64" | "freebsd64" => (114, "freebsd64Guest"),
        "freebsd" | "freebsd-32" => (42, "freebsdGuest"),

        // macOS variants
        "darwin-64" | "darwin64" | "macos" | "darwin" => (101, "darwin64Guest"),

        // Other Linux
        "linux-64" | "other-linux-64" | "otherlinux-64" => (101, "otherLinux64Guest"),
        "linux" | "other-linux" | "otherlinux" => (36, "otherLinuxGuest"),

        // Generic/Other
        "other-64" | "other64" => (102, "other64Guest"),
        _ => (1, "otherGuest"),
    }
}

/// Escape special XML characters in a string.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Sanitize a string to be used as an XML ID.
///
/// Replaces spaces and special characters with underscores.
fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
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
    use std::collections::HashMap;

    fn create_test_config() -> VmxConfig {
        VmxConfig {
            display_name: "TestVM".to_string(),
            guest_os: "ubuntu-64".to_string(),
            memory_mb: 4096,
            num_cpus: 2,
            disks: vec![crate::vmx::DiskConfig {
                file_name: "disk.vmdk".to_string(),
                controller: "scsi0".to_string(),
                unit: 0,
            }],
            networks: vec![crate::vmx::NetworkConfig {
                name: "ethernet0".to_string(),
                virtual_dev: Some("vmxnet3".to_string()),
                network_name: Some("NAT".to_string()),
            }],
            raw: HashMap::new(),
        }
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("hello"), "hello");
        assert_eq!(escape_xml("<test>"), "&lt;test&gt;");
        assert_eq!(escape_xml("a&b"), "a&amp;b");
        assert_eq!(escape_xml("\"quote\""), "&quot;quote&quot;");
        assert_eq!(escape_xml("it's"), "it&apos;s");
    }

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("TestVM"), "TestVM");
        assert_eq!(sanitize_id("Test VM"), "Test_VM");
        assert_eq!(sanitize_id("VM<>123"), "VM__123");
        assert_eq!(sanitize_id("my-vm_01"), "my-vm_01");
    }

    #[test]
    fn test_map_guest_os_ubuntu() {
        let (id, os_type) = map_guest_os("ubuntu-64");
        assert_eq!(id, 96);
        assert_eq!(os_type, "ubuntu64Guest");
    }

    #[test]
    fn test_map_guest_os_windows() {
        let (id, os_type) = map_guest_os("windows10-64");
        assert_eq!(id, 109);
        assert_eq!(os_type, "windows9_64Guest");
    }

    #[test]
    fn test_map_guest_os_unknown() {
        let (id, os_type) = map_guest_os("unknownOS");
        assert_eq!(id, 1);
        assert_eq!(os_type, "otherGuest");
    }

    #[test]
    fn test_ovf_builder_new() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);
        assert_eq!(builder.config.display_name, "TestVM");
    }

    #[test]
    fn test_build_basic_ovf() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);
        let disks = vec![DiskInfo {
            id: "vmdisk1".to_string(),
            file_ref: "file1".to_string(),
            capacity_bytes: 10 * 1024 * 1024 * 1024,
            file_size_bytes: 1024 * 1024 * 100,
        }];

        let ovf = builder.build(&disks).unwrap();

        // Verify XML declaration
        assert!(ovf.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));

        // Verify namespaces
        assert!(ovf.contains("xmlns:ovf="));
        assert!(ovf.contains("xmlns:rasd="));
        assert!(ovf.contains("xmlns:vssd="));
        assert!(ovf.contains("xmlns:vmw="));
        assert!(ovf.contains("xmlns:xsi="));
    }

    #[test]
    fn test_build_envelope_open() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);
        let envelope = builder.build_envelope_open();

        assert!(envelope.contains("ovf:Envelope"));
        assert!(envelope.contains("http://schemas.dmtf.org/ovf/envelope/1"));
    }

    #[test]
    fn test_build_references() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);
        let disks = vec![DiskInfo {
            id: "vmdisk1".to_string(),
            file_ref: "file1".to_string(),
            capacity_bytes: 10737418240,
            file_size_bytes: 104857600,
        }];

        let refs = builder.build_references(&disks);
        assert!(refs.contains("ovf:References"));
        assert!(refs.contains("ovf:File"));
        assert!(refs.contains("ovf:href=\"disk.vmdk\""));
        assert!(refs.contains("ovf:id=\"file1\""));
        assert!(refs.contains("ovf:size=\"104857600\""));
    }

    #[test]
    fn test_build_disk_section() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);
        let disks = vec![DiskInfo {
            id: "vmdisk1".to_string(),
            file_ref: "file1".to_string(),
            capacity_bytes: 10737418240,
            file_size_bytes: 104857600,
        }];

        let section = builder.build_disk_section(&disks);
        assert!(section.contains("ovf:DiskSection"));
        assert!(section.contains("ovf:Disk"));
        assert!(section.contains("ovf:diskId=\"vmdisk1\""));
        assert!(section.contains("ovf:capacity=\"10737418240\""));
        assert!(section.contains("vmdk.html#streamOptimized"));
    }

    #[test]
    fn test_build_network_section() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);

        let section = builder.build_network_section();
        assert!(section.contains("ovf:NetworkSection"));
        assert!(section.contains("ovf:Network"));
        assert!(section.contains("ovf:name=\"NAT\""));
    }

    #[test]
    fn test_build_network_section_default() {
        let mut config = create_test_config();
        config.networks.clear();
        let builder = OvfBuilder::new(&config);

        let section = builder.build_network_section();
        assert!(section.contains("ovf:NetworkSection"));
        assert!(section.contains("ovf:name=\"VM Network\""));
    }

    #[test]
    fn test_build_virtual_system() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);
        let disks = vec![DiskInfo {
            id: "vmdisk1".to_string(),
            file_ref: "file1".to_string(),
            capacity_bytes: 10737418240,
            file_size_bytes: 104857600,
        }];

        let vs = builder.build_virtual_system(&disks);
        assert!(vs.contains("ovf:VirtualSystem"));
        assert!(vs.contains("ovf:id=\"TestVM\""));
        assert!(vs.contains("<ovf:Name>TestVM</ovf:Name>"));
    }

    #[test]
    fn test_build_hardware_section() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);
        let disks = vec![DiskInfo {
            id: "vmdisk1".to_string(),
            file_ref: "file1".to_string(),
            capacity_bytes: 10737418240,
            file_size_bytes: 104857600,
        }];

        let hw = builder.build_hardware_section(&disks);
        assert!(hw.contains("ovf:VirtualHardwareSection"));

        // CPU (ResourceType=3)
        assert!(hw.contains("<rasd:ResourceType>3</rasd:ResourceType>"));
        assert!(hw.contains("<rasd:VirtualQuantity>2</rasd:VirtualQuantity>"));

        // Memory (ResourceType=4)
        assert!(hw.contains("<rasd:ResourceType>4</rasd:ResourceType>"));
        assert!(hw.contains("<rasd:VirtualQuantity>4096</rasd:VirtualQuantity>"));

        // SCSI Controller (ResourceType=6)
        assert!(hw.contains("<rasd:ResourceType>6</rasd:ResourceType>"));

        // Disk (ResourceType=17)
        assert!(hw.contains("<rasd:ResourceType>17</rasd:ResourceType>"));

        // Network (ResourceType=10)
        assert!(hw.contains("<rasd:ResourceType>10</rasd:ResourceType>"));
    }

    #[test]
    fn test_build_cpu_item() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);

        let cpu = builder.build_cpu_item();
        assert!(cpu.contains("<rasd:ResourceType>3</rasd:ResourceType>"));
        assert!(cpu.contains("<rasd:VirtualQuantity>2</rasd:VirtualQuantity>"));
        assert!(cpu.contains("hertz * 10^6"));
    }

    #[test]
    fn test_build_memory_item() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);

        let mem = builder.build_memory_item();
        assert!(mem.contains("<rasd:ResourceType>4</rasd:ResourceType>"));
        assert!(mem.contains("<rasd:VirtualQuantity>4096</rasd:VirtualQuantity>"));
        assert!(mem.contains("byte * 2^20"));
    }

    #[test]
    fn test_build_scsi_controller() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);

        let scsi = builder.build_scsi_controller();
        assert!(scsi.contains("<rasd:ResourceType>6</rasd:ResourceType>"));
        assert!(scsi.contains("lsilogic"));
        assert!(scsi.contains("SCSI Controller 0"));
    }

    #[test]
    fn test_disk_id_with_special_characters_escaped() {
        let config = create_test_config();
        let builder = OvfBuilder::new(&config);

        // Test disk ID with special XML characters
        let disks = vec![DiskInfo {
            id: "disk<>&\"'1".to_string(),
            file_ref: "file1".to_string(),
            capacity_bytes: 10737418240,
            file_size_bytes: 104857600,
        }];

        let ovf = builder.build(&disks).unwrap();

        // Verify that special characters in disk ID are properly escaped in HostResource
        assert!(ovf.contains("ovf:/disk/disk&lt;&gt;&amp;&quot;&apos;1"));
        // Verify they are also escaped in DiskSection
        assert!(ovf.contains("ovf:diskId=\"disk&lt;&gt;&amp;&quot;&apos;1\""));
    }

    #[test]
    fn test_adapter_type_with_special_characters_escaped() {
        let mut config = create_test_config();
        // Modify network adapter type to contain special characters
        if let Some(network) = config.networks.first_mut() {
            network.virtual_dev = Some("E1000<script>".to_string());
        }

        let builder = OvfBuilder::new(&config);
        let disks = vec![DiskInfo {
            id: "vmdisk1".to_string(),
            file_ref: "file1".to_string(),
            capacity_bytes: 10737418240,
            file_size_bytes: 104857600,
        }];

        let ovf = builder.build(&disks).unwrap();

        // Verify that special characters in adapter type are properly escaped
        assert!(ovf.contains("E1000&lt;script&gt;"));
    }
}
