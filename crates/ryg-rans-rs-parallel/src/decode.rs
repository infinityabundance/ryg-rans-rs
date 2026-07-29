//! # Parallel block decoding
//!
//! Supports two strategies:
//!
//! - **Seekable**: A planning pass validates the complete container structure,
//!   then workers decode disjoint block ranges into disjoint output regions.
//! - **Streaming**: Sequential container reader feeds a bounded queue; workers
//!   decode concurrently and the reorder buffer emits blocks in order.

use crate::config::ParallelConfig;
use crate::error::ParallelError;
use crate::job::{DecodeBlockJob, DecodedBlockResult, OrderedDecodedBlocks};

/// Parallel block decoder.
pub struct ParallelDecoder;

impl ParallelDecoder {
    /// Decode blocks from a seekable container (all block data available).
    pub fn decode_blocks(
        _blocks: impl IntoIterator<Item = DecodeBlockJob>,
        _config: &ParallelConfig,
    ) -> Result<OrderedDecodedBlocks, ParallelError> {
        Err(ParallelError::Internal(
            "ParallelDecoder not yet implemented".into(),
        ))
    }

    /// Decode blocks from a streaming container (blocks arrive sequentially,
    /// decoded in parallel, emitted in order).
    pub fn decode_streaming(
        _blocks: impl IntoIterator<Item = DecodeBlockJob>,
        _config: &ParallelConfig,
    ) -> Result<OrderedDecodedBlocks, ParallelError> {
        Err(ParallelError::Internal(
            "ParallelDecoder::decode_streaming not yet implemented".into(),
        ))
    }
}
