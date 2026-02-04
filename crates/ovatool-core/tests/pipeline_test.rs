//! Integration tests for the parallel processing pipeline.

use ovatool_core::pipeline::{
    CompressionLevel, Pipeline, PipelineConfig, PipelineProgress, ProgressTracker,
};

#[test]
fn test_pipeline_processes_chunks() {
    // Process 4 chunks, verify all processed and count matches
    let pipeline = Pipeline::default();
    let chunks: Vec<Vec<u8>> = vec![
        vec![1, 2, 3, 4],
        vec![5, 6, 7, 8],
        vec![9, 10, 11, 12],
        vec![13, 14, 15, 16],
    ];

    let results: Vec<u32> = pipeline
        .process(chunks, |_idx, data| {
            // Sum all bytes in the chunk
            let sum: u32 = data.iter().map(|&b| b as u32).sum();
            Ok(sum)
        })
        .expect("Processing should succeed");

    // Verify all chunks were processed
    assert_eq!(results.len(), 4, "Should have 4 results");

    // Verify the sums are correct (order preserved)
    assert_eq!(results[0], 1 + 2 + 3 + 4);
    assert_eq!(results[1], 5 + 6 + 7 + 8);
    assert_eq!(results[2], 9 + 10 + 11 + 12);
    assert_eq!(results[3], 13 + 14 + 15 + 16);
}

#[test]
fn test_pipeline_preserves_order() {
    // Process 10 chunks with index markers, verify output order matches input
    let pipeline = Pipeline::default();

    // Create chunks where each chunk contains its index as the first byte
    let chunks: Vec<Vec<u8>> = (0..10)
        .map(|i| {
            let mut chunk = vec![i as u8]; // Index marker
            chunk.extend(vec![0u8; 99]); // Padding to make 100 bytes
            chunk
        })
        .collect();

    let results: Vec<u8> = pipeline
        .process(chunks, |idx, data| {
            // Verify the chunk data matches the expected index
            assert_eq!(
                data[0], idx as u8,
                "Chunk data should match its original index"
            );
            // Return the index marker from the data
            Ok(data[0])
        })
        .expect("Processing should succeed");

    // Verify all 10 chunks were processed
    assert_eq!(results.len(), 10, "Should have 10 results");

    // Verify the order is preserved - each result should match its position
    for (expected_idx, &actual_value) in results.iter().enumerate() {
        assert_eq!(
            actual_value, expected_idx as u8,
            "Result at position {} should be {}, but got {}",
            expected_idx, expected_idx, actual_value
        );
    }
}

#[test]
fn test_compression_level_variants() {
    assert_eq!(CompressionLevel::Fast.to_zlib_level(), 1);
    assert_eq!(CompressionLevel::Balanced.to_zlib_level(), 6);
    assert_eq!(CompressionLevel::Max.to_zlib_level(), 9);
}

#[test]
fn test_pipeline_config_accessors() {
    let config = PipelineConfig::new(65536, CompressionLevel::Max, 4);
    let pipeline = Pipeline::new(config);

    assert_eq!(pipeline.chunk_size(), 65536);
    assert_eq!(pipeline.compression_level(), 9);
}

#[test]
fn test_progress_tracking_integration() {
    let config = PipelineConfig::new(1024, CompressionLevel::Balanced, 2);
    let pipeline = Pipeline::new(config);
    let tracker = ProgressTracker::new(5, 500);

    let chunks: Vec<Vec<u8>> = vec![
        vec![0; 100],
        vec![0; 100],
        vec![0; 100],
        vec![0; 100],
        vec![0; 100],
    ];

    let _results: Vec<usize> = pipeline
        .process_with_progress(
            chunks,
            |_idx, data| {
                // Simulate compression: output is half the input size
                let compressed_size = data.len() / 2;
                Ok((data.len(), compressed_size as u64))
            },
            &tracker,
        )
        .expect("Processing should succeed");

    assert!(tracker.is_complete());

    let progress = tracker.snapshot();
    assert_eq!(progress.total_chunks, 5);
    assert_eq!(progress.processed_chunks, 5);
    assert_eq!(progress.total_bytes, 500);
    assert_eq!(progress.processed_bytes, 500);
    assert_eq!(progress.compressed_bytes, 250);
    assert_eq!(progress.percent_complete(), 100.0);
    assert_eq!(progress.compression_ratio(), 0.5);
}

#[test]
fn test_pipeline_progress_methods() {
    let mut progress = PipelineProgress::new(100, 10000);

    // Initially 0%
    assert_eq!(progress.percent_complete(), 0.0);
    assert_eq!(progress.compression_ratio(), 1.0); // No bytes processed

    // After processing 50 chunks
    progress.processed_chunks = 50;
    progress.processed_bytes = 5000;
    progress.compressed_bytes = 2500;

    assert_eq!(progress.percent_complete(), 50.0);
    assert_eq!(progress.compression_ratio(), 0.5);

    // After processing all chunks
    progress.processed_chunks = 100;
    progress.processed_bytes = 10000;
    progress.compressed_bytes = 4000;

    assert_eq!(progress.percent_complete(), 100.0);
    assert_eq!(progress.compression_ratio(), 0.4);
}

#[test]
fn test_parallel_processing_with_multiple_threads() {
    // Test with explicit thread count
    let config = PipelineConfig::new(1024, CompressionLevel::Fast, 4);
    let pipeline = Pipeline::new(config);

    // Create 100 chunks to ensure parallel processing
    let chunks: Vec<Vec<u8>> = (0..100).map(|i| vec![i as u8; 10]).collect();

    let results: Vec<u8> = pipeline
        .process(chunks, |_idx, data| Ok(data[0]))
        .expect("Processing should succeed");

    assert_eq!(results.len(), 100);

    // Verify order is still preserved even with parallel processing
    for (i, &val) in results.iter().enumerate() {
        assert_eq!(val, i as u8);
    }
}

#[test]
fn test_empty_chunks() {
    let pipeline = Pipeline::default();
    let chunks: Vec<Vec<u8>> = vec![];

    let results: Vec<u8> = pipeline
        .process(chunks, |_idx, _data| Ok(0u8))
        .expect("Empty processing should succeed");

    assert!(results.is_empty());
}

#[test]
fn test_error_handling() {
    let pipeline = Pipeline::default();
    let chunks: Vec<Vec<u8>> = vec![vec![1], vec![2], vec![3]];

    let result: ovatool_core::Result<Vec<u8>> = pipeline.process(chunks, |idx, _data| {
        if idx == 1 {
            Err(ovatool_core::Error::pipeline("intentional test error"))
        } else {
            Ok(0)
        }
    });

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("intentional test error"));
}
