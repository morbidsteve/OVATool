//! Export pipeline orchestration.
//!
//! This module coordinates the multi-threaded export process,
//! managing the flow from VMX parsing through OVA creation.

// TODO: Implement pipeline
// - Coordinate parallel VMDK processing
// - Manage memory budget for grain caching
// - Handle progress reporting
// - Implement graceful cancellation
