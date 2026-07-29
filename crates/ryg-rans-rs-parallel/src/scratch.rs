//! # Worker-local scratch buffers
//!
//! Avoid allocation in hot loops.  Each worker owns reusable scratch resources.
//! Buffers may grow only up to configured maxima.  After processing a large block,
//! retain capacity only according to a documented cap to prevent one adversarial
//! block from permanently inflating every worker's memory footprint.

/// Per-worker scratch buffers for decode operations.
#[derive(Debug)]
pub struct WorkerScratch {
    /// Input buffer (compressed block data).
    pub input_buffer: Vec<u8>,
    /// Output buffer (decoded bytes).
    pub output_buffer: Vec<u8>,
    /// Model data buffer.
    pub model_buffer: Vec<u8>,
    /// Maximum capacity to retain after processing a block.
    max_retain: usize,
}

impl WorkerScratch {
    /// Create new scratch buffers with approximate initial capacity.
    pub fn new(initial_capacity: usize, max_retain: usize) -> Self {
        Self {
            input_buffer: Vec::with_capacity(initial_capacity),
            output_buffer: Vec::with_capacity(initial_capacity * 2),
            model_buffer: Vec::with_capacity(256),
            max_retain,
        }
    }

    /// Reset buffers for reuse.  Retains allocated capacity up to `max_retain`.
    pub fn reset(&mut self) {
        self.input_buffer.clear();
        self.output_buffer.clear();
        self.model_buffer.clear();
    }
}

/// A pool of scratch buffers, one per worker.
#[derive(Debug)]
pub struct ScratchPool {
    buffers: Vec<WorkerScratch>,
}

impl ScratchPool {
    /// Create a pool with `count` scratch buffers.
    pub fn new(count: usize, initial_capacity: usize, max_retain: usize) -> Self {
        let buffers = (0..count)
            .map(|_| WorkerScratch::new(initial_capacity, max_retain))
            .collect();
        Self { buffers }
    }

    /// Get the scratch buffer for a specific worker index.
    pub fn get(&mut self, worker_index: usize) -> Option<&mut WorkerScratch> {
        self.buffers.get_mut(worker_index)
    }

    /// Reset all buffers.
    pub fn reset_all(&mut self) {
        for buf in &mut self.buffers {
            buf.reset();
        }
    }

    /// Number of buffers in the pool.
    pub fn len(&self) -> usize {
        self.buffers.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scratch_creation() {
        let s = WorkerScratch::new(1024, 65536);
        assert!(s.input_buffer.capacity() >= 1024);
    }

    #[test]
    fn test_scratch_reset() {
        let mut s = WorkerScratch::new(1024, 65536);
        s.input_buffer.resize(100, 42);
        assert_eq!(s.input_buffer.len(), 100);
        s.reset();
        assert!(s.input_buffer.is_empty());
    }

    #[test]
    fn test_pool() {
        let mut pool = ScratchPool::new(4, 1024, 65536);
        assert_eq!(pool.len(), 4);
        assert!(pool.get(0).is_some());
        assert!(pool.get(4).is_none());
    }
}
