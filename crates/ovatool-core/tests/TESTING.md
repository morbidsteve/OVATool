# Integration Testing

This directory contains integration tests for the OVATool export pipeline.

## Quick Start

```bash
# Run unit tests only (no fixtures required)
cargo test --test integration_test

# Run all tests including those requiring fixtures
cargo test --test integration_test -- --include-ignored
```

## Test Fixtures

Integration tests that require real VM files are marked with `#[ignore]`. To run these tests, you need to set up test fixtures.

### Required Fixture Files

Create the following files in `tests/fixtures/test-vm/`:

1. **test.vmx** - VMX configuration file
2. **test.vmdk** - VMDK descriptor file
3. **test-flat.vmdk** - Flat extent file (actual disk data)

### Creating Minimal Fixtures

For a minimal test setup with a 1MB disk:

```bash
# Navigate to test fixtures directory
cd crates/ovatool-core/tests/fixtures/test-vm

# Create VMX file
cat > test.vmx << 'EOF'
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "21"
displayName = "IntegrationTestVM"
guestOS = "ubuntu-64"
memsize = "2048"
numvcpus = "2"
scsi0.present = "TRUE"
scsi0.virtualDev = "lsilogic"
scsi0:0.present = "TRUE"
scsi0:0.fileName = "test.vmdk"
ethernet0.present = "TRUE"
ethernet0.virtualDev = "e1000"
ethernet0.networkName = "NAT"
EOF

# Create VMDK descriptor
cat > test.vmdk << 'EOF'
# Disk DescriptorFile
version=1
CID=fffffffe
parentCID=ffffffff
createType="monolithicFlat"

# Extent description
RW 2048 FLAT "test-flat.vmdk" 0

# The Disk Data Base
ddb.virtualHWVersion = "21"
ddb.geometry.cylinders = "1"
ddb.geometry.heads = "16"
ddb.geometry.sectors = "63"
ddb.adapterType = "lsilogic"
EOF

# Create 1MB flat disk file (2048 sectors * 512 bytes)
dd if=/dev/zero of=test-flat.vmdk bs=512 count=2048
```

### Using Real VM Files

You can also copy files from an actual VMware VM:
- Copy the `.vmx` file
- Copy the `.vmdk` descriptor file
- Copy the `-flat.vmdk` or data extent file

Note: Large disk files will work but will take longer to process during tests.

## Test Categories

### Unit Tests (No Fixtures Required)

These tests run without fixtures and verify basic functionality:
- `test_export_options_*` - Verify export option configurations
- `test_export_phase_display` - Verify phase enum display
- `test_export_progress_*` - Verify progress tracking
- `test_compression_level_values` - Verify compression level mappings

### Integration Tests (Require Fixtures)

These tests require VM fixtures and test the full pipeline:
- `test_full_export_pipeline` - Complete VMX to OVA export
- `test_export_with_progress_callback` - Progress callback invocation
- `test_export_creates_valid_manifest` - SHA256 manifest validation
- `test_export_compression_level_*` - Different compression levels
- `test_ova_structure_verification` - OVA file structure
- `test_ova_tar_format_compliance` - TAR format compliance
- `test_get_vm_info*` - VM info extraction

## Running Specific Tests

```bash
# Run a specific test
cargo test --test integration_test test_full_export_pipeline -- --ignored

# Run all tests with verbose output
cargo test --test integration_test -- --include-ignored --nocapture

# Run tests in release mode (faster compression)
cargo test --test integration_test --release -- --include-ignored
```

## Troubleshooting

### "test fixture not found" Error

The test will print this message if fixture files are missing. Follow the fixture setup instructions above.

### Export Failed Errors

Check that:
1. All three fixture files exist (test.vmx, test.vmdk, test-flat.vmdk)
2. The VMDK descriptor correctly references the flat file
3. File permissions allow reading
4. The flat file size matches the extent size in the descriptor
