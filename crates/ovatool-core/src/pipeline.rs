//! Export pipeline orchestration.
//!
//! This module coordinates the multi-threaded export process,
//! managing the flow from VMX parsing through OVA creation.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rayon::prelude::*;

use crate::error::{Error, Result};

/// Compression level for VMDK stream optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionLevel {
    /// Fast compression (zlib level 1).
    Fast,
    /// Balanced compression (zlib level 6).
    #[default]
    Balanced,
    /// Maximum compression (zlib level 9).
    Max,
}

impl CompressionLevel {
    /// Convert to zlib compression level.
    pub fn to_zlib_level(&self) -> u32 {
        match self {
            CompressionLevel::Fast => 1,
            CompressionLevel::Balanced => 6,
            CompressionLevel::Max => 9,
        }
    }
}

/// Configuration for the export pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Size of each chunk to process in bytes.
    pub chunk_size: usize,
    /// Compression level for output.
    pub compression_level: CompressionLevel,
    /// Number of threads to use. 0 means use rayon's default (usually number of CPUs).
    pub num_threads: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunk_size: 1024 * 1024, // 1 MB default
            compression_level: CompressionLevel::default(),
            num_threads: 0, // Use rayon's default
        }
    }
}

impl PipelineConfig {
    /// Create a new pipeline configuration.
    pub fn new(chunk_size: usize, compression_level: CompressionLevel, num_threads: usize) -> Self {
        Self {
            chunk_size,
            compression_level,
            num_threads,
        }
    }
}

/// Progress information for the pipeline.
#[derive(Debug, Clone, Default)]
pub struct PipelineProgress {
    /// Total number of chunks to process.
    pub total_chunks: usize,
    /// Number of chunks processed so far.
    pub processed_chunks: usize,
    /// Total bytes to process.
    pub total_bytes: u64,
    /// Bytes processed so far.
    pub processed_bytes: u64,
    /// Bytes after compression.
    pub compressed_bytes: u64,
}

impl PipelineProgress {
    /// Create a new progress tracker with initial values.
    pub fn new(total_chunks: usize, total_bytes: u64) -> Self {
        Self {
            total_chunks,
            processed_chunks: 0,
            total_bytes,
            processed_bytes: 0,
            compressed_bytes: 0,
        }
    }

    /// Calculate the percentage complete.
    pub fn percent_complete(&self) -> f64 {
        if self.total_chunks == 0 {
            return 100.0;
        }
        (self.processed_chunks as f64 / self.total_chunks as f64) * 100.0
    }

    /// Calculate the compression ratio.
    /// Returns the ratio of compressed bytes to processed bytes.
    /// A ratio less than 1.0 means compression is effective.
    pub fn compression_ratio(&self) -> f64 {
        if self.processed_bytes == 0 {
            return 1.0;
        }
        self.compressed_bytes as f64 / self.processed_bytes as f64
    }
}

/// Thread-safe progress tracker for the pipeline.
#[derive(Debug, Clone)]
pub struct ProgressTracker {
    progress: Arc<Mutex<PipelineProgress>>,
}

impl ProgressTracker {
    /// Create a new progress tracker.
    pub fn new(total_chunks: usize, total_bytes: u64) -> Self {
        Self {
            progress: Arc::new(Mutex::new(PipelineProgress::new(total_chunks, total_bytes))),
        }
    }

    /// Update progress after processing a chunk.
    pub fn update(&self, input_bytes: u64, output_bytes: u64) {
        let mut progress = self.progress.lock().unwrap();
        progress.processed_chunks += 1;
        progress.processed_bytes += input_bytes;
        progress.compressed_bytes += output_bytes;
    }

    /// Get a snapshot of the current progress.
    pub fn snapshot(&self) -> PipelineProgress {
        self.progress.lock().unwrap().clone()
    }

    /// Check if processing is complete.
    pub fn is_complete(&self) -> bool {
        let progress = self.progress.lock().unwrap();
        progress.processed_chunks >= progress.total_chunks
    }
}

/// The parallel processing pipeline.
#[derive(Debug)]
pub struct Pipeline {
    config: PipelineConfig,
    thread_pool: Option<rayon::ThreadPool>,
}

impl Pipeline {
    /// Create a new pipeline with the given configuration.
    pub fn new(config: PipelineConfig) -> Self {
        let thread_pool = if config.num_threads > 0 {
            Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(config.num_threads)
                    .build()
                    .expect("Failed to build thread pool"),
            )
        } else {
            None
        };

        Self {
            config,
            thread_pool,
        }
    }

    /// Get the zlib compression level.
    pub fn compression_level(&self) -> u32 {
        self.config.compression_level.to_zlib_level()
    }

    /// Get the configured chunk size.
    pub fn chunk_size(&self) -> usize {
        self.config.chunk_size
    }

    /// Process chunks in parallel using the provided processor function.
    ///
    /// The processor function receives the chunk index and data, and returns
    /// a result. Results are reordered to match the input order.
    ///
    /// # Arguments
    ///
    /// * `chunks` - Vector of byte chunks to process
    /// * `processor` - Function to process each chunk, receives (index, data)
    ///
    /// # Returns
    ///
    /// Vector of processed results in the same order as input chunks.
    pub fn process<F, T>(&self, chunks: Vec<Vec<u8>>, processor: F) -> Result<Vec<T>>
    where
        F: Fn(usize, Vec<u8>) -> Result<T> + Send + Sync,
        T: Send,
    {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        // Process chunks in parallel and collect results with their indices
        let process_indexed = |chunks: Vec<Vec<u8>>| -> Result<Vec<T>> {
            // Create indexed chunks
            let indexed_chunks: Vec<(usize, Vec<u8>)> =
                chunks.into_iter().enumerate().collect();

            // Process in parallel, collecting (index, result) pairs
            let results: std::result::Result<BTreeMap<usize, T>, Error> = indexed_chunks
                .into_par_iter()
                .map(|(idx, chunk)| {
                    processor(idx, chunk).map(|result| (idx, result))
                })
                .collect();

            // Convert BTreeMap to Vec (BTreeMap maintains order by key)
            let ordered_map = results?;
            Ok(ordered_map.into_values().collect())
        };

        // Use custom thread pool if configured, otherwise use global pool
        match &self.thread_pool {
            Some(pool) => pool.install(|| process_indexed(chunks)),
            None => process_indexed(chunks),
        }
    }

    /// Process chunks with progress tracking.
    ///
    /// Same as `process` but updates a progress tracker.
    pub fn process_with_progress<F, T>(
        &self,
        chunks: Vec<Vec<u8>>,
        processor: F,
        tracker: &ProgressTracker,
    ) -> Result<Vec<T>>
    where
        F: Fn(usize, Vec<u8>) -> Result<(T, u64)> + Send + Sync,
        T: Send,
    {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        let process_indexed = |chunks: Vec<Vec<u8>>| -> Result<Vec<T>> {
            let indexed_chunks: Vec<(usize, Vec<u8>)> =
                chunks.into_iter().enumerate().collect();

            let results: std::result::Result<BTreeMap<usize, T>, Error> = indexed_chunks
                .into_par_iter()
                .map(|(idx, chunk)| {
                    let input_len = chunk.len() as u64;
                    let (result, output_len) = processor(idx, chunk)?;
                    tracker.update(input_len, output_len);
                    Ok((idx, result))
                })
                .collect();

            let ordered_map = results?;
            Ok(ordered_map.into_values().collect())
        };

        match &self.thread_pool {
            Some(pool) => pool.install(|| process_indexed(chunks)),
            None => process_indexed(chunks),
        }
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new(PipelineConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_level_to_zlib() {
        assert_eq!(CompressionLevel::Fast.to_zlib_level(), 1);
        assert_eq!(CompressionLevel::Balanced.to_zlib_level(), 6);
        assert_eq!(CompressionLevel::Max.to_zlib_level(), 9);
    }

    #[test]
    fn test_compression_level_default() {
        assert_eq!(CompressionLevel::default(), CompressionLevel::Balanced);
    }

    #[test]
    fn test_pipeline_config_default() {
        let config = PipelineConfig::default();
        assert_eq!(config.chunk_size, 1024 * 1024);
        assert_eq!(config.compression_level, CompressionLevel::Balanced);
        assert_eq!(config.num_threads, 0);
    }

    #[test]
    fn test_pipeline_config_new() {
        let config = PipelineConfig::new(4096, CompressionLevel::Max, 4);
        assert_eq!(config.chunk_size, 4096);
        assert_eq!(config.compression_level, CompressionLevel::Max);
        assert_eq!(config.num_threads, 4);
    }

    #[test]
    fn test_pipeline_progress_percent_complete() {
        let mut progress = PipelineProgress::new(10, 1000);
        assert_eq!(progress.percent_complete(), 0.0);

        progress.processed_chunks = 5;
        assert_eq!(progress.percent_complete(), 50.0);

        progress.processed_chunks = 10;
        assert_eq!(progress.percent_complete(), 100.0);
    }

    #[test]
    fn test_pipeline_progress_percent_complete_empty() {
        let progress = PipelineProgress::new(0, 0);
        assert_eq!(progress.percent_complete(), 100.0);
    }

    #[test]
    fn test_pipeline_progress_compression_ratio() {
        let mut progress = PipelineProgress::new(10, 1000);
        assert_eq!(progress.compression_ratio(), 1.0); // No bytes processed yet

        progress.processed_bytes = 1000;
        progress.compressed_bytes = 500;
        assert_eq!(progress.compression_ratio(), 0.5); // 50% compression

        progress.compressed_bytes = 1000;
        assert_eq!(progress.compression_ratio(), 1.0); // No compression
    }

    #[test]
    fn test_progress_tracker() {
        let tracker = ProgressTracker::new(4, 400);

        assert!(!tracker.is_complete());

        tracker.update(100, 50);
        let snap = tracker.snapshot();
        assert_eq!(snap.processed_chunks, 1);
        assert_eq!(snap.processed_bytes, 100);
        assert_eq!(snap.compressed_bytes, 50);

        tracker.update(100, 60);
        tracker.update(100, 40);
        tracker.update(100, 50);

        assert!(tracker.is_complete());

        let final_snap = tracker.snapshot();
        assert_eq!(final_snap.processed_chunks, 4);
        assert_eq!(final_snap.processed_bytes, 400);
        assert_eq!(final_snap.compressed_bytes, 200);
        assert_eq!(final_snap.percent_complete(), 100.0);
        assert_eq!(final_snap.compression_ratio(), 0.5);
    }

    #[test]
    fn test_pipeline_compression_level() {
        let config = PipelineConfig::new(1024, CompressionLevel::Max, 0);
        let pipeline = Pipeline::new(config);
        assert_eq!(pipeline.compression_level(), 9);
    }

    #[test]
    fn test_pipeline_chunk_size() {
        let config = PipelineConfig::new(4096, CompressionLevel::Fast, 0);
        let pipeline = Pipeline::new(config);
        assert_eq!(pipeline.chunk_size(), 4096);
    }

    #[test]
    fn test_pipeline_process_empty() {
        let pipeline = Pipeline::default();
        let chunks: Vec<Vec<u8>> = vec![];
        let results: Vec<usize> = pipeline
            .process(chunks, |idx, _data| Ok(idx))
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_pipeline_processes_chunks() {
        let pipeline = Pipeline::default();
        let chunks: Vec<Vec<u8>> = vec![
            vec![1, 2, 3],
            vec![4, 5, 6],
            vec![7, 8, 9],
            vec![10, 11, 12],
        ];

        let results: Vec<usize> = pipeline
            .process(chunks, |_idx, data| Ok(data.len()))
            .unwrap();

        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|&len| len == 3));
    }

    #[test]
    fn test_pipeline_preserves_order() {
        let pipeline = Pipeline::default();
        let chunks: Vec<Vec<u8>> = (0..10)
            .map(|i| vec![i as u8])
            .collect();

        let results: Vec<u8> = pipeline
            .process(chunks, |_idx, data| Ok(data[0]))
            .unwrap();

        assert_eq!(results.len(), 10);
        for (i, &val) in results.iter().enumerate() {
            assert_eq!(val, i as u8, "Order not preserved at index {}", i);
        }
    }

    #[test]
    fn test_pipeline_with_custom_threads() {
        let config = PipelineConfig::new(1024, CompressionLevel::Balanced, 2);
        let pipeline = Pipeline::new(config);

        let chunks: Vec<Vec<u8>> = vec![
            vec![1],
            vec![2],
            vec![3],
            vec![4],
        ];

        let results: Vec<u8> = pipeline
            .process(chunks, |_idx, data| Ok(data[0] * 2))
            .unwrap();

        assert_eq!(results, vec![2, 4, 6, 8]);
    }

    #[test]
    fn test_pipeline_error_propagation() {
        let pipeline = Pipeline::default();
        let chunks: Vec<Vec<u8>> = vec![
            vec![1],
            vec![2],
            vec![3],
        ];

        let result: Result<Vec<u8>> = pipeline.process(chunks, |idx, _data| {
            if idx == 1 {
                Err(Error::pipeline("test error"))
            } else {
                Ok(0)
            }
        });

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_pipeline_with_progress_tracking() {
        let pipeline = Pipeline::default();
        let tracker = ProgressTracker::new(3, 300);

        let chunks: Vec<Vec<u8>> = vec![
            vec![0; 100],
            vec![0; 100],
            vec![0; 100],
        ];

        let results: Vec<usize> = pipeline
            .process_with_progress(chunks, |_idx, data| {
                let compressed_size = data.len() / 2;
                Ok((data.len(), compressed_size as u64))
            }, &tracker)
            .unwrap();

        assert_eq!(results.len(), 3);
        assert!(tracker.is_complete());

        let snap = tracker.snapshot();
        assert_eq!(snap.processed_bytes, 300);
        assert_eq!(snap.compressed_bytes, 150);
        assert_eq!(snap.compression_ratio(), 0.5);
    }
}
