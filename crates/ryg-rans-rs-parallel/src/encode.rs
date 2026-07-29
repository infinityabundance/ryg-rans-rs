//! # Parallel per-block encoding with ordered commit
//!
//! Uses the bounded executor to encode blocks in parallel, then commits
//! them in block-index order through the reorder buffer.

use crate::config::{CodecPolicy, ModelPolicy, ParallelConfig};
use crate::error::ParallelError;
use crate::job::{EncodeBlockJob, EncodedBlockResult, OrderedEncodedBlocks};
use crate::plan::FixedBlockPlan;
use crate::reorder::{BufferSized, HasBlockIndex, ReorderBuffer};

impl HasBlockIndex for EncodedBlockResult {
    fn block_index(&self) -> u64 {
        self.block_index
    }
}

impl BufferSized for EncodedBlockResult {
    fn buffer_size(&self) -> u64 {
        self.block.len() as u64 + 128 // 128 bytes overhead
    }
}

/// Parallel block encoder.
pub struct ParallelEncoder;

impl ParallelEncoder {
    /// Encode blocks in parallel using the given configuration.
    ///
    /// Returns blocks in ascending block-index order, ready for container serialisation.
    pub fn encode_blocks(
        _blocks: impl IntoIterator<Item = EncodeBlockJob>,
        _config: &ParallelConfig,
    ) -> Result<OrderedEncodedBlocks, ParallelError> {
        // TODO: Implement actual block encoding using ParallelExecutor
        Err(ParallelError::Internal(
            "ParallelEncoder not yet implemented".into(),
        ))
    }

    /// Encode blocks with a pre-computed fixed block plan.
    pub fn encode_planned(
        _plan: &FixedBlockPlan,
        _data: &[u8],
        _config: &ParallelConfig,
    ) -> Result<OrderedEncodedBlocks, ParallelError> {
        Err(ParallelError::Internal(
            "ParallelEncoder::encode_planned not yet implemented".into(),
        ))
    }
}
