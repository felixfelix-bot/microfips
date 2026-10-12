//! Length-prefixed fragment header for the ESP-NOW fragmentation pipeline.
//!
//! # Wire format (6 bytes, big-endian)
//!
//! ```text
//!  offset  size  field
//!  ------  ----  -----------------------------------------------------------
//!     0     2    block_id        identifies the block this fragment belongs to
//!     2     1    frag_index      original: 0..original_count  (0-based)
//!                                 redundancy: original_count..original_count+budget
//!     3     1    original_count  k, the number of original fragments in the block
//!     4     2    crc16           CRC-16/CCITT-FALSE over the fragment payload
//! ```
//!
//! `original_count` travels in every fragment on purpose: a receiver that
//! catches only redundancy fragments still learns `k`, and therefore the
//! parity-matrix geometry, without waiting for an original to arrive.

use alloc::vec::Vec;

/// Size of the fragment header on the wire.
pub const HEADER_SIZE: usize = 6;

/// Smallest fragment payload that makes sense (header only).
pub const MIN_FRAGMENT_SIZE: usize = HEADER_SIZE;

/// CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF, no reflection, no final XOR).
///
/// Matches balloon-fresh's frame CRC so fragments can be validated
/// identically on both sides.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// A parsed fragment header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FragmentHeader {
    /// Which block this fragment belongs to.
    pub block_id: u16,
    /// 0-based fragment index within the block.
    pub frag_index: u8,
    /// `k`: how many original fragments the block was split into.
    pub original_count: u8,
    /// CRC-16 over the fragment payload that follows the header.
    pub crc: u16,
}

impl FragmentHeader {
    /// True when this fragment is a redundancy (parity) fragment, not an
    /// original.
    pub fn is_redundancy(&self) -> bool {
        self.frag_index >= self.original_count
    }

    /// Serialize to the 6-byte wire form.
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut out = [0u8; HEADER_SIZE];
        out[0..2].copy_from_slice(&self.block_id.to_be_bytes());
        out[2] = self.frag_index;
        out[3] = self.original_count;
        out[4..6].copy_from_slice(&self.crc.to_be_bytes());
        out
    }

    /// Parse the 6-byte wire form. Returns `None` on a short or nonsensical
    /// header (zero `original_count`, or an index that cannot exist).
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < HEADER_SIZE {
            return None;
        }
        let header = FragmentHeader {
            block_id: u16::from_be_bytes([buf[0], buf[1]]),
            frag_index: buf[2],
            original_count: buf[3],
            crc: u16::from_be_bytes([buf[4], buf[5]]),
        };
        // `original_count == 0` would make every fragment look like redundancy
        // and give the decoder a zero-width matrix; reject it up front.
        if header.original_count == 0 {
            return None;
        }
        Some(header)
    }

    /// Verify `payload` against this header's CRC.
    pub fn verify(&self, payload: &[u8]) -> bool {
        crc16(payload) == self.crc
    }
}

/// A fully-encoded fragment: header plus payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub header: FragmentHeader,
    pub payload: Vec<u8>,
}

impl Fragment {
    /// Build a fragment, computing the CRC over `payload`.
    pub fn new(block_id: u16, frag_index: u8, original_count: u8, payload: &[u8]) -> Self {
        Self {
            header: FragmentHeader {
                block_id,
                frag_index,
                original_count,
                crc: crc16(payload),
            },
            payload: payload.to_vec(),
        }
    }

    /// Serialize header + payload into one wire buffer.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_SIZE + self.payload.len());
        out.extend_from_slice(&self.header.to_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    /// Parse a whole wire fragment. Returns `None` on a short buffer or a CRC
    /// mismatch — a corrupt fragment is dropped, never repaired.
    pub fn parse(buf: &[u8]) -> Option<Self> {
        let header = FragmentHeader::parse(buf)?;
        let payload = buf.get(HEADER_SIZE..)?;
        if !header.verify(payload) {
            return None;
        }
        Some(Self {
            header,
            payload: payload.to_vec(),
        })
    }
}
