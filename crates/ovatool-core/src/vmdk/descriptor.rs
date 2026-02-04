//! VMDK descriptor file parsing.
//!
//! This module handles parsing VMDK descriptor files to extract disk metadata,
//! extent information, and geometry settings.

use crate::error::{Error, Result};

/// The type of a VMDK extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtentType {
    /// Flat extent - raw disk data in a separate file.
    Flat,
    /// Sparse extent - uses grain tables for allocation.
    Sparse,
    /// Zero extent - represents zeroed data without storage.
    Zero,
    /// VMFS extent - VMware VMFS filesystem.
    Vmfs,
    /// VMFS sparse extent.
    VmfsSparse,
    /// VMFS raw device mapping.
    VmfsRdm,
    /// VMFS raw extent.
    VmfsRaw,
}

impl ExtentType {
    /// Parse an extent type from a string.
    fn from_str(s: &str) -> Result<Self> {
        match s.to_uppercase().as_str() {
            "FLAT" => Ok(ExtentType::Flat),
            "SPARSE" => Ok(ExtentType::Sparse),
            "ZERO" => Ok(ExtentType::Zero),
            "VMFS" => Ok(ExtentType::Vmfs),
            "VMFSSPARSE" => Ok(ExtentType::VmfsSparse),
            "VMFSRDM" => Ok(ExtentType::VmfsRdm),
            "VMFSRAW" => Ok(ExtentType::VmfsRaw),
            _ => Err(Error::vmdk(format!("unknown extent type: {}", s))),
        }
    }
}

/// A VMDK extent entry describing a portion of the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extent {
    /// Access mode (e.g., "RW" for read-write, "RDONLY" for read-only).
    pub access: String,
    /// Size of this extent in 512-byte sectors.
    pub size_sectors: u64,
    /// Type of the extent.
    pub extent_type: ExtentType,
    /// Filename of the extent file.
    pub filename: String,
    /// Offset within the extent file (in sectors).
    pub offset: u64,
}

/// Parsed VMDK descriptor containing disk metadata.
#[derive(Debug, Clone)]
pub struct VmdkDescriptor {
    /// Descriptor format version.
    pub version: u32,
    /// Content ID for change tracking.
    pub cid: u32,
    /// Parent content ID for delta disks.
    pub parent_cid: u32,
    /// The type of VMDK (e.g., "monolithicFlat", "twoGbMaxExtentSparse").
    pub create_type: String,
    /// List of extent entries.
    pub extents: Vec<Extent>,
    /// Disk geometry: cylinders.
    pub cylinders: u64,
    /// Disk geometry: heads.
    pub heads: u32,
    /// Disk geometry: sectors per track.
    pub sectors: u32,
    /// Virtual hardware version.
    pub hw_version: String,
    /// Disk adapter type (e.g., "lsilogic", "ide", "buslogic").
    pub adapter_type: String,
}

impl VmdkDescriptor {
    /// Calculate the total disk size in bytes.
    pub fn disk_size_bytes(&self) -> u64 {
        self.disk_size_sectors() * 512
    }

    /// Calculate the total disk size in sectors.
    pub fn disk_size_sectors(&self) -> u64 {
        self.extents.iter().map(|e| e.size_sectors).sum()
    }
}

/// Parse a VMDK descriptor from its text content.
///
/// # Arguments
///
/// * `content` - The text content of the VMDK descriptor file.
///
/// # Returns
///
/// A `VmdkDescriptor` containing the parsed metadata.
///
/// # Errors
///
/// Returns an error if the descriptor format is invalid or required fields are missing.
pub fn parse_descriptor(content: &str) -> Result<VmdkDescriptor> {
    let mut version = 1;
    let mut cid = 0u32;
    let mut parent_cid = 0xffffffffu32;
    let mut create_type = String::new();
    let mut extents = Vec::new();
    let mut cylinders = 0u64;
    let mut heads = 0u32;
    let mut sectors = 0u32;
    let mut hw_version = String::new();
    let mut adapter_type = String::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Check for extent line (starts with access mode)
        if line.starts_with("RW ") || line.starts_with("RDONLY ") || line.starts_with("NOACCESS ") {
            let extent = parse_extent_line(line)?;
            extents.push(extent);
            continue;
        }

        // Parse key=value or key = value pairs
        if let Some((key, value)) = parse_key_value(line) {
            match key.as_str() {
                "version" => {
                    version = value
                        .parse()
                        .map_err(|_| Error::vmdk(format!("invalid version: {}", value)))?;
                }
                "CID" => {
                    cid = u32::from_str_radix(&value, 16)
                        .map_err(|_| Error::vmdk(format!("invalid CID: {}", value)))?;
                }
                "parentCID" => {
                    parent_cid = u32::from_str_radix(&value, 16)
                        .map_err(|_| Error::vmdk(format!("invalid parentCID: {}", value)))?;
                }
                "createType" => {
                    create_type = value;
                }
                "ddb.virtualHWVersion" => {
                    hw_version = value;
                }
                "ddb.geometry.cylinders" => {
                    cylinders = value
                        .parse()
                        .map_err(|_| Error::vmdk(format!("invalid cylinders: {}", value)))?;
                }
                "ddb.geometry.heads" => {
                    heads = value
                        .parse()
                        .map_err(|_| Error::vmdk(format!("invalid heads: {}", value)))?;
                }
                "ddb.geometry.sectors" => {
                    sectors = value
                        .parse()
                        .map_err(|_| Error::vmdk(format!("invalid sectors: {}", value)))?;
                }
                "ddb.adapterType" => {
                    adapter_type = value;
                }
                _ => {
                    // Ignore unknown keys
                }
            }
        }
    }

    Ok(VmdkDescriptor {
        version,
        cid,
        parent_cid,
        create_type,
        extents,
        cylinders,
        heads,
        sectors,
        hw_version,
        adapter_type,
    })
}

/// Parse a key=value or key = value line.
///
/// Returns None if the line doesn't contain an equals sign.
fn parse_key_value(line: &str) -> Option<(String, String)> {
    let eq_pos = line.find('=')?;
    let key = line[..eq_pos].trim().to_string();
    let mut value = line[eq_pos + 1..].trim().to_string();

    // Remove surrounding quotes if present
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        value = value[1..value.len() - 1].to_string();
    }

    Some((key, value))
}

/// Parse an extent line like: "RW 838860800 FLAT "TestVM-flat.vmdk" 0"
fn parse_extent_line(line: &str) -> Result<Extent> {
    // Extent format: ACCESS SIZE TYPE "FILENAME" OFFSET
    // The filename is quoted, so we need to handle that specially

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 {
        return Err(Error::vmdk(format!("invalid extent line: {}", line)));
    }

    let access = parts[0].to_string();
    let size_sectors: u64 = parts[1]
        .parse()
        .map_err(|_| Error::vmdk(format!("invalid extent size: {}", parts[1])))?;
    let extent_type = ExtentType::from_str(parts[2])?;

    // Find the quoted filename - it could span multiple "parts" if filename has spaces
    let rest_of_line = line
        .split_whitespace()
        .skip(3)
        .collect::<Vec<&str>>()
        .join(" ");

    let (filename, offset_str) = parse_quoted_filename_and_offset(&rest_of_line)?;

    let offset: u64 = offset_str
        .parse()
        .map_err(|_| Error::vmdk(format!("invalid extent offset: {}", offset_str)))?;

    Ok(Extent {
        access,
        size_sectors,
        extent_type,
        filename,
        offset,
    })
}

/// Parse a quoted filename followed by an offset from a string like: "filename.vmdk" 0
fn parse_quoted_filename_and_offset(s: &str) -> Result<(String, String)> {
    let s = s.trim();

    if !s.starts_with('"') {
        return Err(Error::vmdk(format!("expected quoted filename, got: {}", s)));
    }

    // Find the closing quote
    let end_quote = s[1..]
        .find('"')
        .ok_or_else(|| Error::vmdk(format!("unclosed quote in: {}", s)))?
        + 1;

    let filename = s[1..end_quote].to_string();
    let offset_str = s[end_quote + 1..].trim().to_string();

    Ok((filename, offset_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extent_type_from_str() {
        assert_eq!(ExtentType::from_str("FLAT").unwrap(), ExtentType::Flat);
        assert_eq!(ExtentType::from_str("flat").unwrap(), ExtentType::Flat);
        assert_eq!(ExtentType::from_str("SPARSE").unwrap(), ExtentType::Sparse);
        assert_eq!(ExtentType::from_str("ZERO").unwrap(), ExtentType::Zero);
        assert_eq!(ExtentType::from_str("VMFS").unwrap(), ExtentType::Vmfs);
        assert_eq!(
            ExtentType::from_str("VMFSSPARSE").unwrap(),
            ExtentType::VmfsSparse
        );
        assert_eq!(
            ExtentType::from_str("VMFSRDM").unwrap(),
            ExtentType::VmfsRdm
        );
        assert_eq!(
            ExtentType::from_str("VMFSRAW").unwrap(),
            ExtentType::VmfsRaw
        );
    }

    #[test]
    fn test_extent_type_unknown() {
        assert!(ExtentType::from_str("UNKNOWN").is_err());
    }

    #[test]
    fn test_parse_key_value_no_spaces() {
        let (key, value) = parse_key_value("version=1").unwrap();
        assert_eq!(key, "version");
        assert_eq!(value, "1");
    }

    #[test]
    fn test_parse_key_value_with_spaces() {
        let (key, value) = parse_key_value("ddb.geometry.cylinders = \"52216\"").unwrap();
        assert_eq!(key, "ddb.geometry.cylinders");
        assert_eq!(value, "52216");
    }

    #[test]
    fn test_parse_key_value_quoted() {
        let (key, value) = parse_key_value("createType=\"monolithicFlat\"").unwrap();
        assert_eq!(key, "createType");
        assert_eq!(value, "monolithicFlat");
    }

    #[test]
    fn test_parse_extent_line() {
        let extent = parse_extent_line("RW 838860800 FLAT \"TestVM-flat.vmdk\" 0").unwrap();
        assert_eq!(extent.access, "RW");
        assert_eq!(extent.size_sectors, 838860800);
        assert_eq!(extent.extent_type, ExtentType::Flat);
        assert_eq!(extent.filename, "TestVM-flat.vmdk");
        assert_eq!(extent.offset, 0);
    }

    #[test]
    fn test_parse_extent_line_sparse() {
        let extent = parse_extent_line("RW 12345 SPARSE \"disk.vmdk\" 128").unwrap();
        assert_eq!(extent.access, "RW");
        assert_eq!(extent.size_sectors, 12345);
        assert_eq!(extent.extent_type, ExtentType::Sparse);
        assert_eq!(extent.filename, "disk.vmdk");
        assert_eq!(extent.offset, 128);
    }

    #[test]
    fn test_parse_quoted_filename_and_offset() {
        let (filename, offset) = parse_quoted_filename_and_offset("\"disk.vmdk\" 0").unwrap();
        assert_eq!(filename, "disk.vmdk");
        assert_eq!(offset, "0");
    }

    #[test]
    fn test_parse_quoted_filename_with_spaces() {
        let (filename, offset) =
            parse_quoted_filename_and_offset("\"my disk file.vmdk\" 128").unwrap();
        assert_eq!(filename, "my disk file.vmdk");
        assert_eq!(offset, "128");
    }

    #[test]
    fn test_disk_size_calculations() {
        let descriptor = VmdkDescriptor {
            version: 1,
            cid: 0,
            parent_cid: 0xffffffff,
            create_type: "test".to_string(),
            extents: vec![
                Extent {
                    access: "RW".to_string(),
                    size_sectors: 1000,
                    extent_type: ExtentType::Flat,
                    filename: "a.vmdk".to_string(),
                    offset: 0,
                },
                Extent {
                    access: "RW".to_string(),
                    size_sectors: 2000,
                    extent_type: ExtentType::Flat,
                    filename: "b.vmdk".to_string(),
                    offset: 0,
                },
            ],
            cylinders: 0,
            heads: 0,
            sectors: 0,
            hw_version: String::new(),
            adapter_type: String::new(),
        };

        assert_eq!(descriptor.disk_size_sectors(), 3000);
        assert_eq!(descriptor.disk_size_bytes(), 3000 * 512);
    }
}
