//! Transmit half of the fragmentation pipeline: pad a frame, split it into
//! `block_size` originals, erasure-encode `redundancy` parity fragments, and
//! emit every fragment to a caller-supplied sink.
//!
//! # Why a callback instead of a returned `Vec<Vec<u8>>`
//!
//! A transmitting leaf node hands each fragment to a radio driver as soon as it
//! exists; it never needs the whole block resident at once, and on a `no_std`
//! target it may have nowhere to put it. [`PipelineTx::send_frame`] therefore
//! borrows the encoder directly from the caller's frame buffer and pushes each
//! fragment through [`FragmentSink::emit`] in wire order, releasing it
//! immediately. The total number of live fragment buffers is one.
//!
//! # Padding
//!
//! The erasure codec is defined over a fixed geometry: exactly
//! `block_size * frag_size` payload bytes, split into `block_size` equal
//! fragments. A frame shorter than that is zero-padded to the block boundary
//! ([`pad_into_block`]) and the original length is carried on the side
//! ([`PaddedFrame::payload_len`]) so the receiver can trim it back. A frame
//! *longer* than one block is rejected, not truncated -- silently dropping the
//! tail of a frame is a data-loss bug dressed up as a success.
//!
//! Because trailing zeros are indistinguishable from zero padding, a frame
//! whose real payload ends in `0x00` cannot be recovered exactly by trimming.
//! Callers that need byte-exact round-trips of arbitrary binary frames must
//! either length-delimit above this layer or keep such frames off the tail;
//! the RX side's [`crate::trim_padding`] documents the same constraint.

use alloc::vec::Vec;

use crate::erasure::{ErasureEncoder, MAX_FRAG_SIZE, MAX_FRAGS, MAX_REDUNDANCY};
use crate::fragment::{Fragment, HEADER_SIZE};
use crate::PipelineConfig;

/// Where emitted fragments go.
///
/// Implemented by the transport (a radio driver, a test collector, ...). Called
/// exactly once per fragment, in emission order (all originals, then all parity
/// fragments), with the complete wire image: 6-byte header followed by
/// `frag_size` payload bytes.
///
/// Returning `Err` stops the block immediately: `send_frame` propagates the
/// error and emits no further fragments. A sink that has already emitted part
/// of a block must tolerate that -- the block is simply incomplete on the wire
/// and the receiver will not decode it unless enough parity fragments arrived.
pub trait FragmentSink {
    /// The error type a sink reports. Kept as an associated type so the
    /// pipeline does not force a transport-specific error (an `io::Error` on a
    /// host, a radio status enum on a leaf node) into its own signature.
    type Error;

    /// Consume one wire fragment.
    fn emit(&mut self, fragment: &[u8]) -> Result<(), Self::Error>;
}

/// Adapter: collect fragments into a `Vec<Vec<u8>>`.
///
/// Handy for tests and for callers that really do want the whole block; it is
/// deliberately not how the transport path is meant to work.
#[derive(Debug, Default)]
pub struct VecSink {
    pub fragments: Vec<Vec<u8>>,
}

impl VecSink {
    pub fn new() -> Self {
        Self {
            fragments: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.fragments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }
}

impl FragmentSink for VecSink {
    /// Infallible: collection cannot fail.
    type Error = core::convert::Infallible;

    fn emit(&mut self, fragment: &[u8]) -> Result<(), Self::Error> {
        self.fragments.push(fragment.to_vec());
        Ok(())
    }
}

/// Adapter: count fragments and drop their bytes.
///
/// Used by the throughput path when the caller wants the encoding work done
/// without retaining any fragment.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CountingSink {
    pub count: usize,
    pub bytes: usize,
}

impl FragmentSink for CountingSink {
    type Error = core::convert::Infallible;

    fn emit(&mut self, fragment: &[u8]) -> Result<(), Self::Error> {
        self.count += 1;
        self.bytes += fragment.len();
        Ok(())
    }
}

/// The transmit half of the pipeline.
///
/// Holds only the geometry and the rolling `block_id` counter, so it needs no
/// scratch buffers and is safe to keep in a leaf node's hot path.
pub struct Pipeline {
    config: PipelineConfig,
    next_block_id: u16,
}

impl Pipeline {
    /// Build a transmitter for `config`, or `None` if the geometry is invalid
    /// (see [`PipelineConfig::validate`]).
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

    /// The `block_id` the next call to [`Self::send_frame`] will use.
    pub fn next_block_id(&self) -> u16 {
        self.next_block_id
    }

    /// Payload bytes one block carries; a longer frame is rejected.
    pub fn capacity(&self) -> usize {
        self.config.block_payload_capacity()
    }

    /// Fragment the frame in `scratch`, encoding it into `k` originals plus
    /// `redundancy` parity fragments and emitting each through `sink`.
    ///
    /// `scratch` must be at least [`Self::capacity`] bytes; only its first
    /// `capacity` bytes are used, and it is overwritten (the frame is laid down
    /// at the front and zero-padded to the block boundary). Borrowing the
    /// caller's buffer rather than allocating is what lets this run on a node
    /// with no allocator in the radio path.
    ///
    /// On success returns the `block_id` that was stamped into every fragment.
    /// The counter advances even if the sink fails part-way, so a failed block
    /// id is never reused for different content (a receiver that saw the first
    /// fragments of a failed block would otherwise mix them with a later,
    /// unrelated block carrying the same id).
    pub fn send_frame<E>(
        &mut self,
        data: &[u8],
        scratch: &mut [u8],
        sink: &mut impl FragmentSink<Error = E>,
    ) -> Result<u16, TxError<E>> {
        let capacity = self.capacity();
        if scratch.len() < capacity {
            return Err(TxError::ScratchTooSmall {
                need: capacity,
                got: scratch.len(),
            });
        }
        if data.len() > capacity {
            return Err(TxError::FrameTooLong {
                capacity,
                got: data.len(),
            });
        }

        let block = pad_into_block(data, &mut scratch[..capacity]);

        // Claim the id before doing any work so a sink error cannot rewind it.
        let block_id = self.next_block_id;
        self.next_block_id = self.next_block_id.wrapping_add(1);

        let encoder =
            ErasureEncoder::new(self.config.block_size, self.config.frag_size, block)
                .ok_or(TxError::BadGeometry)?;

        let mut wire = Vec::with_capacity(self.config.wire_fragment_size());

        // Originals first: a receiver that only catches the leading fragments
        // still has whole originals and needs no erasure decoding at all.
        for idx in 0..self.config.block_size {
            let off = idx * self.config.frag_size;
            let payload = &block[off..off + self.config.frag_size];
            let frag = Fragment::new(block_id, idx as u8, self.config.block_size as u8, payload);
            wire.clear();
            wire.extend_from_slice(&frag.header.to_bytes());
            wire.extend_from_slice(payload);
            sink.emit(&wire).map_err(TxError::Sink)?;
        }

        // Then parity, indexed from `block_size` so `frag_index >= k` marks it
        // redundancy on the wire.
        let mut parity = [0u8; MAX_FRAG_SIZE];
        for r in 0..self.config.redundancy {
            if !encoder.encode_redundant(r, &mut parity[..self.config.frag_size]) {
                return Err(TxError::BadGeometry);
            }
            let frag = Fragment::new(
                block_id,
                (self.config.block_size + r) as u8,
                self.config.block_size as u8,
                &parity[..self.config.frag_size],
            );
            wire.clear();
            wire.extend_from_slice(&frag.header.to_bytes());
            wire.extend_from_slice(&parity[..self.config.frag_size]);
            sink.emit(&wire).map_err(TxError::Sink)?;
        }

        let _ = HEADER_SIZE;
        Ok(block_id)
    }
}

/// Lay `data` down at the front of `block` and zero-fill the rest.
///
/// Returns the block slice so callers cannot accidentally encode past it.
/// `block` must be exactly one block long; the caller guarantees that via
/// [`Pipeline::capacity`].
pub fn pad_into_block<'b>(data: &[u8], block: &'b mut [u8]) -> &'b mut [u8] {
    let n = data.len().min(block.len());
    block[..n].copy_from_slice(&data[..n]);
    for b in block[n..].iter_mut() {
        *b = 0;
    }
    block
}

/// Why a frame could not be transmitted.
///
/// Deliberately not `std::io::Error`: this crate is `no_std` and runs on a
/// leaf node with no `io` layer. The sink's own error is carried verbatim in
/// [`TxError::Sink`] so no transport-specific failure has to be flattened into
/// a string or an errno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxError<E> {
    /// The caller's scratch buffer was shorter than one block.
    ScratchTooSmall { need: usize, got: usize },
    /// The frame did not fit in one block; it is rejected, never truncated.
    FrameTooLong { capacity: usize, got: usize },
    /// The erasure codec rejected the geometry or a parity index.
    BadGeometry,
    /// The sink refused a fragment; emission stopped at that fragment.
    Sink(E),
}

impl<E: core::fmt::Debug> core::fmt::Display for TxError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TxError::ScratchTooSmall { need, got } => {
                write!(f, "scratch buffer too small: need {need}, got {got}")
            }
            TxError::FrameTooLong { capacity, got } => {
                write!(f, "frame too long: capacity {capacity}, got {got}")
            }
            TxError::BadGeometry => write!(f, "erasure codec rejected the geometry"),
            TxError::Sink(e) => write!(f, "sink rejected a fragment: {e:?}"),
        }
    }
}

#[cfg(feature = "std")]
impl<E: core::fmt::Debug> std::error::Error for TxError<E> {}

/// Geometry sanity for the transmit path, checked once at construction.
///
/// Kept separate from [`PipelineConfig::validate`] because the TX path has one
/// extra constraint: every emitted `frag_index` must be distinguishable as
/// original or redundancy, which needs `block_size + redundancy <= u8::MAX`.
/// `validate` already enforces that, so this is an assertion helper for tests
/// and for callers building a config by struct literal.
pub fn tx_geometry_ok(config: &PipelineConfig) -> bool {
    config.validate()
        && config.block_size <= MAX_FRAGS
        && config.redundancy <= MAX_REDUNDANCY
        && config.frag_size <= MAX_FRAG_SIZE
        && config.frag_size + HEADER_SIZE <= u8::MAX as usize
}
