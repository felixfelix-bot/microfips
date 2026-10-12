//! Fragmentation + erasure-coding pipeline: TX (`PipelineTx`) and RX
//! (`PipelineRx`).
//!
//! Splits an application frame into `k` original fragments, adds `redundancy`
//! parity fragments from the PRBS23-XOR erasure encoder, and puts each on the
//! wire with a 6-byte header ([`crate::fragment`]). On receive, fragments are
//! collected per `block_id`, fed to the erasure decoder in arrival order, and
//! the block is handed back with padding trimmed once the decoder reports
//! complete.
//!
//! Loss tolerance is `redundancy` fragments per block, regardless of *which*
//! ones go missing: a block of 10 originals plus 4 parity fragments survives
//! any 4 lost fragments, originals included.
//!
//! # Dialect note
//!
//! This pipeline is the ESP-NOW-side fragmentation layer. It is independent of
//! `microfips-esp-common::espnow_frag`, which is the *legacy* cross-checked
//! chunking codec used by the currently-shipped ESP-NOW gateway. Both can
//! coexist; nothing here changes the existing transport.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod erasure;
pub mod fragment;
pub mod tx;

/// Debug switch for the GE solver trace, settable from tests/examples.
#[cfg(feature = "std")]
pub static GE_TRACE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[inline]
pub(crate) fn ge_trace_enabled() -> bool {
    #[cfg(feature = "std")]
    {
        GE_TRACE.load(core::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(feature = "std"))]
    {
        false
    }
}

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use crate::erasure::{
    DecodeStatus, ErasureDecoder, ErasureEncoder, MAX_FRAGS, MAX_FRAG_SIZE, MAX_REDUNDANCY,
};
use crate::fragment::{Fragment, HEADER_SIZE};

/// Pipeline geometry and policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineConfig {
    /// `k`: original fragments per block.
    pub block_size: usize,
    /// Payload bytes per fragment (excluding the 6-byte header).
    pub frag_size: usize,
    /// Parity fragments generated per block.
    pub redundancy: usize,
    /// How many partially-received blocks to track at once. Bounds RX memory
    /// on a leaf node; the oldest block is evicted when the table is full.
    pub max_pending_blocks: usize,
}

impl PipelineConfig {
    /// A config with room for 4 concurrently-collected blocks.
    pub fn new(block_size: usize, frag_size: usize, redundancy: usize) -> Self {
        Self {
            block_size,
            frag_size,
            redundancy,
            max_pending_blocks: 4,
        }
    }

    pub fn with_max_pending_blocks(mut self, n: usize) -> Self {
        self.max_pending_blocks = n.max(1);
        self
    }

    /// Validate the geometry against the erasure codec's hard limits.
    ///
    /// `block_size + redundancy` must fit a `u8` because `frag_index` is one
    /// byte on the wire.
    pub fn validate(&self) -> bool {
        self.block_size > 0
            && self.block_size <= MAX_FRAGS
            && self.frag_size > 0
            && self.frag_size <= MAX_FRAG_SIZE
            && self.redundancy > 0
            && self.redundancy <= MAX_REDUNDANCY
            && self.block_size + self.redundancy <= u8::MAX as usize
    }

    /// Bytes of payload one block can carry.
    pub fn block_payload_capacity(&self) -> usize {
        self.block_size * self.frag_size
    }

    /// Total fragments emitted per block (originals + parity).
    pub fn fragments_per_block(&self) -> usize {
        self.block_size + self.redundancy
    }

    /// Encoded size of one fragment on the wire.
    pub fn wire_fragment_size(&self) -> usize {
        HEADER_SIZE + self.frag_size
    }

    /// Highest `frag_index` a block will use.
    pub fn max_frag_index(&self) -> u8 {
        (self.fragments_per_block() - 1) as u8
    }

    /// Scratch bytes the decoder needs for one block's assembly buffer.
    fn assembly_len(&self) -> usize {
        self.block_size * self.frag_size + MAX_REDUNDANCY * self.frag_size
    }

    /// Scratch bytes the decoder needs for one block's parity matrix.
    fn matrix_len(&self) -> usize {
        MAX_REDUNDANCY * ((self.block_size >> 3) + 1)
    }
}

/// A payload plus the length of its meaningful prefix.
///
/// Padding is the gap between "how big the erasure block had to be" and "how
/// big the application frame actually was".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaddedFrame<'a> {
    /// Block-sized buffer (`block_size * frag_size` bytes).
    pub data: &'a [u8],
    /// Bytes of `data` that are real payload; the rest is zero padding.
    pub payload_len: usize,
}

impl<'a> PaddedFrame<'a> {
    /// Wrap a frame whose whole length is meaningful (already block-aligned).
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            payload_len: data.len(),
        }
    }

    /// Wrap a padded frame, recording the real payload length.
    pub fn padded(data: &'a [u8], payload_len: usize) -> Option<Self> {
        if payload_len > data.len() {
            return None;
        }
        Some(Self { data, payload_len })
    }

    /// Bytes of padding at the tail.
    pub fn padding_len(&self) -> usize {
        self.data.len().saturating_sub(self.payload_len)
    }
}

/// Transmit half: encodes frames into fragments.
///
/// Stateless apart from the block-id counter, so it needs no scratch buffers.
pub struct PipelineTx {
    config: PipelineConfig,
    next_block_id: u16,
}

impl PipelineTx {
    pub fn new(config: PipelineConfig) -> Option<Self> {
        if !config.validate() {
            return None;
        }
        Some(Self {
            config,
            next_block_id: 0,
        })
    }

    pub fn config(&self) -> PipelineConfig {
        self.config
    }

    /// Split and encode `frame`, writing all fragments into `out`.
    ///
    /// `out` is cleared first. Returns the `block_id` used.
    ///
    /// Originals are emitted before parity so a receiver that only catches the
    /// first few fragments still gets whole originals and needs no decode work.
    pub fn send_frame(
        &mut self,
        frame: PaddedFrame<'_>,
        out: &mut Vec<Vec<u8>>,
    ) -> Result<u16, PipelineError> {
        out.clear();

        let capacity = self.config.block_payload_capacity();
        if frame.data.len() != capacity {
            return Err(PipelineError::WrongBlockLength {
                expected: capacity,
                got: frame.data.len(),
            });
        }
        if frame.payload_len > capacity {
            return Err(PipelineError::PayloadTooLong {
                capacity,
                got: frame.payload_len,
            });
        }

        let encoder =
            ErasureEncoder::new(self.config.block_size, self.config.frag_size, frame.data)
                .ok_or(PipelineError::BadGeometry)?;

        let block_id = self.next_block_id;
        self.next_block_id = self.next_block_id.wrapping_add(1);

        for idx in 0..self.config.block_size {
            let off = idx * self.config.frag_size;
            let payload = &frame.data[off..off + self.config.frag_size];
            let frag = Fragment::new(block_id, idx as u8, self.config.block_size as u8, payload);
            out.push(frag.to_bytes());
        }

        let mut parity = vec![0u8; self.config.frag_size];
        for r in 0..self.config.redundancy {
            if !encoder.encode_redundant(r, &mut parity) {
                return Err(PipelineError::BadGeometry);
            }
            let frag = Fragment::new(
                block_id,
                (self.config.block_size + r) as u8,
                self.config.block_size as u8,
                &parity,
            );
            out.push(frag.to_bytes());
        }

        Ok(block_id)
    }
}

/// Why a frame could not be encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineError {
    /// The buffer handed to `send_frame` was not exactly one block long.
    WrongBlockLength { expected: usize, got: usize },
    /// The declared payload length exceeded the block capacity.
    PayloadTooLong { capacity: usize, got: usize },
    /// The geometry was rejected by the erasure codec.
    BadGeometry,
}

/// Decode telemetry for a completed block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStats {
    /// `k` for this block.
    pub original_count: usize,
    /// Fragments the pipeline actually fed to the decoder.
    pub frags_received: usize,
    /// True if at least one original never arrived and had to be reconstructed
    /// from parity.
    pub used_redundancy: bool,
}

/// A block that finished decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReassembledBlock {
    /// `block_id` of the completed block.
    pub block_id: u16,
    /// The application frame, padding trimmed.
    pub data: Vec<u8>,
    /// Decode telemetry for this block.
    pub stats: BlockStats,
}

/// Per-block receive state.
struct PendingBlock<'a> {
    decoder: ErasureDecoder<'a>,
    original_count: usize,
    frags_received: usize,
    used_redundancy: bool,
    /// Creation order, used to evict the oldest block first.
    seq: u64,
}

/// Receive half: collects fragments per block and reassembles frames.
pub struct PipelineRx {
    config: PipelineConfig,
    /// Live blocks, keyed by `block_id`.
    pending: BTreeMap<u16, PendingBlock<'static>>,
    /// Scratch allocations, one per trackable block. Owned here for the
    /// lifetime of the pipeline; handed to decoders as `'static` borrows.
    assembly_pool: Vec<&'static mut [u8]>,
    matrix_pool: Vec<&'static mut [u8]>,
    /// Slots currently unused (kept as free-list indices, not buffers).
    free_slots: Vec<usize>,
    next_seq: u64,
    crc_drops: u64,
    evicted_blocks: u64,
}

impl PipelineRx {
    pub fn new(config: PipelineConfig) -> Option<Self> {
        if !config.validate() {
            return None;
        }
        let mut assembly_pool = Vec::with_capacity(config.max_pending_blocks);
        let mut matrix_pool = Vec::with_capacity(config.max_pending_blocks);
        for _ in 0..config.max_pending_blocks {
            assembly_pool.push(vec![0u8; config.assembly_len()].leak());
            matrix_pool.push(vec![0u8; config.matrix_len()].leak());
        }
        let free_slots = (0..config.max_pending_blocks).rev().collect();
        Some(Self {
            config,
            pending: BTreeMap::new(),
            assembly_pool,
            matrix_pool,
            free_slots,
            next_seq: 0,
            crc_drops: 0,
            evicted_blocks: 0,
        })
    }

    pub fn config(&self) -> PipelineConfig {
        self.config
    }

    /// Number of blocks currently being collected.
    pub fn pending_blocks(&self) -> usize {
        self.pending.len()
    }

    /// Fragments rejected by header parsing or CRC validation.
    pub fn crc_drops(&self) -> u64 {
        self.crc_drops
    }

    /// Blocks dropped to make room for newer ones.
    pub fn evicted_blocks(&self) -> u64 {
        self.evicted_blocks
    }

    /// Evict the least-recently-created block, freeing its pool slot.
    fn evict_oldest(&mut self) {
        let oldest = match self.pending.iter().min_by_key(|(_, b)| b.seq) {
            Some((id, _)) => *id,
            None => return,
        };
        if let Some(block) = self.pending.remove(&oldest) {
            let slot = block.decoder.pool_slot();
            self.free_slots.push(slot);
            self.evicted_blocks += 1;
        }
    }

    /// Feed one received fragment.
    ///
    /// Returns `Some(ReassembledBlock)` the moment the block is decodable —
    /// padding trimmed — or `None` if more fragments are needed. Malformed or
    /// corrupt input is dropped and counted, never panicked:
    /// on a `no_std` leaf node a bad radio fragment must be a cheap drop.
    pub fn recv_fragment(&mut self, wire: &[u8]) -> Option<ReassembledBlock> {
        let frag = match Fragment::parse(wire) {
            Some(f) => f,
            None => {
                self.crc_drops += 1;
                return None;
            }
        };

        let k = frag.header.original_count as usize;
        // A fragment claiming a geometry we are not configured for is not ours.
        if k != self.config.block_size {
            return None;
        }
        if frag.payload.len() < self.config.frag_size {
            return None;
        }
        // Reject an index beyond what this block could ever contain.
        let is_redundancy = frag.header.is_redundancy();
        if is_redundancy
            && (frag.header.frag_index as usize - k) >= self.config.redundancy
        {
            return None;
        }

        let block_id = frag.header.block_id;

        if !self.pending.contains_key(&block_id) {
            while self.pending.len() >= self.config.max_pending_blocks {
                let before = self.evicted_blocks;
                self.evict_oldest();
                if self.evicted_blocks == before {
                    break;
                }
            }
            let slot = self.free_slots.pop()?;
            // `mem::take` moves the `&'static mut` out of the Vec without
            // reborrowing it at the local lifetime (indexing would).
            let assembly: &'static mut [u8] = core::mem::take(&mut self.assembly_pool[slot]);
            let matrix: &'static mut [u8] = core::mem::take(&mut self.matrix_pool[slot]);
            // Reset scratch state for reuse.
            for b in assembly.iter_mut() {
                *b = 0;
            }
            for b in matrix.iter_mut() {
                *b = 0;
            }
            let mut decoder =
                ErasureDecoder::new(k, self.config.frag_size, assembly, matrix)?;
            decoder.set_pool_slot(slot);
            let seq = self.next_seq;
            self.next_seq += 1;
            self.pending.insert(
                block_id,
                PendingBlock {
                    decoder,
                    original_count: k,
                    frags_received: 0,
                    used_redundancy: false,
                    seq,
                },
            );
        }

        let block = self.pending.get_mut(&block_id)?;

        // `frag_index` is 0-based in the header; the erasure decoder is 1-based.
        let counter = frag.header.frag_index as u16 + 1;
        let status = block.decoder.process(counter, &frag.payload);
        block.frags_received += 1;
        if is_redundancy {
            block.used_redundancy = true;
        }

        match status {
            DecodeStatus::Complete => {
                let block = self.pending.remove(&block_id)?;
                let slot = block.decoder.pool_slot();
                let data = block.decoder.assembled().map(|s| s.to_vec());
                let stats = BlockStats {
                    original_count: block.original_count,
                    frags_received: block.frags_received,
                    used_redundancy: block.used_redundancy,
                };
                self.free_slots.push(slot);
                let mut data = data?;
                trim_padding(&mut data);
                Some(ReassembledBlock {
                    block_id,
                    data,
                    stats,
                })
            }
            DecodeStatus::Progress | DecodeStatus::Invalid => None,
        }
    }

    /// Drop all pending blocks (e.g. after a link reset).
    pub fn reset(&mut self) {
        let ids: Vec<u16> = self.pending.keys().copied().collect();
        for id in ids {
            if let Some(block) = self.pending.remove(&id) {
                self.free_slots.push(block.decoder.pool_slot());
            }
        }
    }
}

/// The RX entry point named by the task spec: collect fragments for a block and
/// return the reassembled frame once the erasure decoder signals complete,
/// padding trimmed. `None` means "need more fragments".
pub fn recv_frame(rx: &mut PipelineRx, fragment: &[u8]) -> Option<Vec<u8>> {
    rx.recv_fragment(fragment).map(|block| block.data)
}

/// Remove trailing zero padding.
///
/// The erasure block is always `block_size * frag_size` bytes; the application
/// frame inside it may be shorter and zero-padded to that boundary. Trailing
/// zeros are therefore padding by construction.
///
/// A frame whose genuine payload ends in `0x00` is not round-trip exact under
/// this rule. See [`tx`] for the matching note on the transmit side.
pub fn trim_padding(data: &mut Vec<u8>) {
    while data.last() == Some(&0) {
        data.pop();
    }
}

/// Zero-pad `payload` into a fresh block-sized buffer.
///
/// Convenience for callers that have a frame shorter than one block.
pub fn pad_to_block(payload: &[u8], config: &PipelineConfig) -> Option<(Vec<u8>, usize)> {
    let capacity = config.block_payload_capacity();
    if payload.len() > capacity {
        return None;
    }
    let mut buf = vec![0u8; capacity];
    buf[..payload.len()].copy_from_slice(payload);
    Some((buf, payload.len()))
}
