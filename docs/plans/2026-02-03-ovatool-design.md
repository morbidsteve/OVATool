# OVATool Design Document

**Date:** 2026-02-03
**Status:** Approved
**Problem:** VMware OVFTool exports are extremely slow, underutilizing CPU, disk I/O, and RAM

---

## Overview

A multithreaded Rust tool to export VMware Workstation VMs to OVA format significantly faster than OVFTool by parallelizing compression and overlapping I/O operations.

### Requirements

- **Input:** VMware Workstation Pro VMs (monolithic VMDKs via `.vmx` files)
- **Output:** OVA files compatible with VMware products (Workstation, ESXi, vSphere)
- **VM sizes:** 100-500 GB typical
- **Compression:** Balanced (reasonable compression without excessive time)
- **Interface:** CLI core with optional GUI wrapper (future)
- **Language:** Rust

---

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         CLI Interface                            │
│  ovatool export MyVM.vmx -o MyVM.ova --compression=balanced     │
└─────────────────────────────────────────────────────────────────┘
                                │
┌─────────────────────────────────────────────────────────────────┐
│                        Core Library                              │
├─────────────┬─────────────┬─────────────┬──────────────────────┤
│ VMX Parser  │ VMDK Reader │ OVF Builder │ OVA Writer           │
└─────────────┴─────────────┴─────────────┴──────────────────────┘
                                │
┌─────────────────────────────────────────────────────────────────┐
│                    Parallel Pipeline                             │
│  ┌────────┐    ┌────────────┐    ┌──────────┐    ┌───────────┐ │
│  │ Reader │───▶│ Compressor │───▶│ Checksumer│───▶│  Writer   │ │
│  │ Thread │    │ Thread Pool│    │  Thread   │    │  Thread   │ │
│  └────────┘    └────────────┘    └──────────┘    └───────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

**Core components:**

1. **VMX Parser** - Reads VMware's `.vmx` config to find disk paths, VM name, hardware specs
2. **VMDK Reader** - Memory-maps the monolithic VMDK, yields 64MB chunks
3. **OVF Builder** - Generates the XML descriptor (VM config, hardware, disk references)
4. **OVA Writer** - Produces the TAR archive with OVF + compressed disks + manifest

The CLI is a thin wrapper; all logic lives in a library crate so a GUI can be added later.

---

## Parallel Pipeline Design

The pipeline is where we get the speed:

```
VMDK File (400GB)
      │
      ▼
┌─────────────────┐
│  Chunk Reader   │  Single thread, memory-mapped I/O
│  (64MB chunks)  │  Reads ahead into bounded queue
└────────┬────────┘
         │ Channel (bounded, ~8 chunks buffered = 512MB RAM)
         ▼
┌─────────────────┐
│  Compressor     │  Thread pool (num_cpus threads)
│  Pool (rayon)   │  Each chunk: deflate compress
└────────┬────────┘
         │ Channel (preserves chunk order)
         ▼
┌─────────────────┐
│  Checksum +     │  Single thread
│  Writer         │  SHA256 while writing to TAR
└─────────────────┘
         │
         ▼
    MyVM.ova
```

### Key Design Decisions

- **Bounded channels** - Limits memory usage. With 8 chunks buffered at 64MB each, we cap at ~512MB RAM for the pipeline regardless of disk size.

- **Order preservation** - Chunks must be written in order. Compressor threads tag chunks with sequence numbers; writer reorders if needed.

- **Compression format** - VMDK "streamOptimized" format (deflate) for VMware compatibility. Each grain (64KB) is compressed independently, which maps well to our chunking.

- **Checksum streaming** - SHA256 computed incrementally as bytes are written, not as a separate pass.

### Expected Speedup

On an 8-core machine, compression becomes ~6-7x faster. Combined with overlapped I/O, total export should be 4-5x faster than OVFTool for large VMs.

---

## VMDK & OVA Format Details

### Input: Monolithic VMDK

Monolithic VMDKs have two parts:
- **Descriptor** - Small text header with geometry, CID, parent info
- **Flat extent** - Raw disk data (either embedded or in a `-flat.vmdk` file)

Parse the descriptor to get disk geometry, then read the flat extent sequentially.

### Output: StreamOptimized VMDK

VMware OVAs expect "streamOptimized" VMDKs - a chunked, compressed format:

```
┌──────────────┐
│ VMDK Header  │  Sparse header with grain size (64KB typical)
├──────────────┤
│ Grain Table  │  Offsets to each compressed grain
├──────────────┤
│ Grain 0      │  Deflate-compressed 64KB chunk
│ Grain 1      │  ...
│ ...          │
│ Grain N      │
├──────────────┤
│ Footer       │  Copy of header for streaming
└──────────────┘
```

Each 64KB grain is independently compressed with deflate (zlib). This enables random-access after import.

### OVA Structure (TAR)

```
MyVM.ova (TAR archive)
├── MyVM.ovf          # XML descriptor (VM config)
├── MyVM.mf           # SHA256 manifest
└── MyVM-disk1.vmdk   # StreamOptimized disk
```

The manifest contains checksums for the OVF and disk files. VMware validates these on import.

---

## CLI Interface

### Basic Usage

```bash
# Simple export
ovatool export /path/to/MyVM.vmx -o MyVM.ova

# With options
ovatool export MyVM.vmx -o MyVM.ova \
  --compression=fast \
  --threads=8 \
  --progress
```

### Commands

```
ovatool export <vmx-file> -o <output.ova>   # Main export command
ovatool info <vmx-file>                      # Show VM details without exporting
ovatool validate <ova-file>                  # Verify OVA integrity
```

### Export Options

| Flag | Description | Default |
|------|-------------|---------|
| `-o, --output` | Output OVA path | Required |
| `-c, --compression` | `fast`, `balanced`, `max` | `balanced` |
| `-t, --threads` | Worker thread count | num_cpus |
| `--chunk-size` | Pipeline chunk size | `64MB` |
| `--progress` | Show progress bar | enabled if TTY |
| `--quiet` | Suppress output | false |
| `--dry-run` | Parse and validate only | false |

### Compression Presets

- `fast` - zlib level 1, prioritizes speed
- `balanced` - zlib level 6, good tradeoff (default)
- `max` - zlib level 9, smallest output

### Progress Output

```
Exporting: MyVM
  Disk: MyVM.vmdk (427.3 GB)
  [████████████░░░░░░░░] 58% | 248.1 GB | 1.2 GB/s | ETA: 2m 31s
```

---

## Error Handling & Edge Cases

### Errors

| Scenario | Behavior |
|----------|----------|
| VMX file not found | Clear error with path |
| VMDK missing/corrupted | Report which disk, abort |
| Disk full during write | Clean up partial OVA, report space needed |
| Unsupported VMDK type | Error with explanation (e.g., "split disks not yet supported") |
| Permission denied | Suggest running with appropriate permissions |
| VM is running | Warn that export may be inconsistent, require `--force` |

### Edge Cases

- **Snapshots** - If VM has snapshots, export the current state (flattened). Warn the user.
- **Sparse regions** - Monolithic VMDKs may have sparse/zero regions. Detect and compress efficiently (zeros compress extremely well).
- **Multi-disk VMs** - Support VMs with multiple disks; process sequentially (parallel per-disk is future enhancement).
- **Special characters in paths** - Handle spaces, unicode in VM names.
- **Large disks (>2TB)** - Use 64-bit offsets throughout.

### Pre-Export Validation

Before starting the slow work, validate:
1. VMX parses correctly
2. All referenced VMDKs exist and are readable
3. Output path is writable
4. Enough disk space for estimated output (warn if close)

This catches problems in seconds rather than hours into an export.

---

## Project Structure

```
ovatool/
├── Cargo.toml              # Workspace root
├── README.md
├── LICENSE
│
├── crates/
│   ├── ovatool-core/       # Library crate (all the logic)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── vmx.rs      # VMX parser
│   │       ├── vmdk/
│   │       │   ├── mod.rs
│   │       │   ├── reader.rs       # Monolithic VMDK reader
│   │       │   └── stream.rs       # StreamOptimized writer
│   │       ├── ovf.rs      # OVF XML builder
│   │       ├── ova.rs      # TAR archive writer
│   │       ├── pipeline.rs # Parallel chunk pipeline
│   │       └── checksum.rs # SHA256 streaming
│   │
│   └── ovatool-cli/        # Binary crate
│       ├── Cargo.toml
│       └── src/
│           └── main.rs     # CLI with clap
│
└── tests/
    └── integration/        # End-to-end tests with sample VMDKs
```

### Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing |
| `rayon` | Thread pool for compression |
| `flate2` | Deflate compression (zlib) |
| `sha2` | SHA256 checksums |
| `memmap2` | Memory-mapped file I/O |
| `crossbeam-channel` | Bounded channels for pipeline |
| `indicatif` | Progress bars |
| `thiserror` | Error types |

---

## Future Enhancements (Out of Scope)

- GUI wrapper (Tauri or egui)
- Split VMDK support
- Import OVA → VMX
- Parallel multi-disk processing
- Network transfer during export (stream to remote)
