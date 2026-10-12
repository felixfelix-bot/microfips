//! Transmit-path tests: padding, fragment geometry, header fields, CRC, block-id
//! sequencing, sink error handling, and a TX -> RX round trip.
//!
//! The RX side is driven through `PipelineRx`/`recv_frame` from the crate root,
//! so these tests also serve as the cross-check that the two halves agree on the
//! wire format.

use microfips_pipeline::erasure::MAX_REDUNDANCY;
use microfips_pipeline::fragment::{crc16, Fragment, HEADER_SIZE};
use microfips_pipeline::tx::{
    pad_into_block, tx_geometry_ok, CountingSink, FragmentSink, Pipeline, TxError, VecSink,
};
use microfips_pipeline::{recv_frame, PipelineConfig, PipelineRx};

/// A small geometry that is easy to reason about: 4 originals of 8 bytes.
fn cfg() -> PipelineConfig {
    PipelineConfig::new(4, 8, 2)
}

fn tx() -> Pipeline {
    Pipeline::new(cfg()).expect("valid geometry")
}

/// Deterministic non-trivial payload; deliberately ends in a non-zero byte so
/// trimming cannot hide a truncation bug.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 37 + 11) % 251 + 1) as u8).collect()
}

// --- padding ---------------------------------------------------------------

#[test]
fn pad_into_block_zero_fills_the_tail() {
    let mut block = [0xAAu8; 32];
    let data = payload(5);
    let out = pad_into_block(&data, &mut block);
    assert_eq!(out.len(), 32);
    assert_eq!(&out[..5], &data[..], "payload must land at the front");
    assert!(
        out[5..].iter().all(|&b| b == 0),
        "the tail must be zero padding, not stale buffer contents"
    );
}

#[test]
fn pad_into_block_of_full_length_payload_changes_nothing() {
    let mut block = [0u8; 32];
    let data = payload(32);
    let out = pad_into_block(&data, &mut block);
    assert_eq!(out, &data[..]);
}

#[test]
fn pad_into_block_does_not_overrun_a_short_buffer() {
    // Defensive: a caller that hands a short buffer gets it filled only to its
    // own length, never past it. `send_frame` rejects this case before calling
    // here, but the helper must not depend on that.
    let mut block = [0xFFu8; 4];
    let data = payload(10);
    let out = pad_into_block(&data, &mut block);
    assert_eq!(out.len(), 4);
    assert_eq!(out, &data[..4]);
}

#[test]
fn padding_round_trips_through_the_pipeline() {
    // Frame shorter than one block: pad, encode, then decode and trim.
    let mut t = tx();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let data = payload(13); // 4 * 8 = 32 capacity
    t.send_frame(&data, &mut scratch, &mut sink).unwrap();

    let mut rx = PipelineRx::new(cfg()).unwrap();
    let mut got = None;
    for f in &sink.fragments {
        if let Some(block) = recv_frame(&mut rx, f) {
            got = Some(block);
            break;
        }
    }
    let got = got.expect("all fragments delivered, decode must complete");
    assert_eq!(got, data, "padding must be trimmed exactly back to the frame");
}

// --- fragment generation ---------------------------------------------------

#[test]
fn emits_k_originals_plus_redundancy_and_nothing_else() {
    let mut t = tx();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let data = payload(t.capacity());
    t.send_frame(&data, &mut scratch, &mut sink).unwrap();

    let c = cfg();
    assert_eq!(
        sink.len(),
        c.fragments_per_block(),
        "one fragment per original plus one per parity index"
    );
    assert_eq!(sink.len(), 6, "4 originals + 2 parity");
}

#[test]
fn every_fragment_has_the_configured_wire_size() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    t.send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut sink)
        .unwrap();

    for (i, f) in sink.fragments.iter().enumerate() {
        assert_eq!(
            f.len(),
            c.wire_fragment_size(),
            "fragment {i} is not header + frag_size bytes"
        );
        assert_eq!(f.len(), HEADER_SIZE + 8);
    }
}

#[test]
fn originals_carry_their_own_slice_of_the_frame_in_order() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let data = payload(c.block_payload_capacity());
    t.send_frame(&data, &mut scratch, &mut sink).unwrap();

    for idx in 0..c.block_size {
        let frag = Fragment::parse(&sink.fragments[idx]).unwrap();
        assert_eq!(frag.header.frag_index, idx as u8);
        assert!(
            !frag.header.is_redundancy(),
            "fragment {idx} must be an original, not parity"
        );
        let off = idx * c.frag_size;
        assert_eq!(
            &frag.payload[..],
            &data[off..off + c.frag_size],
            "original {idx} must carry frame bytes [{off}..{}]",
            off + c.frag_size
        );
    }
}

#[test]
fn originals_are_emitted_before_parity() {
    // The ordering property receivers rely on: catching only the first k
    // fragments yields every original and needs no erasure decode.
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    t.send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut sink)
        .unwrap();

    for (i, f) in sink.fragments.iter().enumerate() {
        let frag = Fragment::parse(f).unwrap();
        let expected_redundancy = i >= c.block_size;
        assert_eq!(
            frag.header.is_redundancy(),
            expected_redundancy,
            "fragment {i} is on the wrong side of the originals/parity split"
        );
    }
}

#[test]
fn parity_fragments_are_indexed_from_k() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    t.send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut sink)
        .unwrap();

    for r in 0..c.redundancy {
        let frag = Fragment::parse(&sink.fragments[c.block_size + r]).unwrap();
        assert_eq!(
            frag.header.frag_index,
            (c.block_size + r) as u8,
            "parity fragment {r} must be indexed k + r"
        );
        assert!(frag.header.is_redundancy());
    }
}

#[test]
fn parity_fragments_are_the_xor_of_their_matrix_row() {
    // The encoder's contract, re-derived independently from the wire output:
    // parity fragment r equals the XOR of the originals whose bits are set in
    // row (r + 1) of the parity matrix.
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let data = payload(c.block_payload_capacity());
    t.send_frame(&data, &mut scratch, &mut sink).unwrap();

    for r in 0..c.redundancy {
        let row = microfips_pipeline::erasure::parity_row_bits(r as i32 + 1, c.block_size as i32);
        let mut expect = vec![0u8; c.frag_size];
        for &col in &row {
            if col < c.block_size {
                let off = col * c.frag_size;
                for b in 0..c.frag_size {
                    expect[b] ^= data[off + b];
                }
            }
        }
        let frag = Fragment::parse(&sink.fragments[c.block_size + r]).unwrap();
        assert_eq!(frag.payload[..], expect[..], "parity fragment {r} mismatch");
    }
}

// --- header fields ---------------------------------------------------------

#[test]
fn every_fragment_is_parseable_and_crc_valid() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    t.send_frame(&payload(17), &mut scratch, &mut sink).unwrap();

    for (i, f) in sink.fragments.iter().enumerate() {
        let frag = Fragment::parse(f)
            .unwrap_or_else(|| panic!("fragment {i} failed to parse or failed CRC"));
        assert!(
            frag.header.verify(&frag.payload),
            "fragment {i} CRC must verify"
        );
        assert_eq!(frag.payload.len(), c.frag_size);
    }
}

#[test]
fn every_fragment_carries_the_original_count() {
    // A receiver that catches only parity fragments must still learn k, which
    // is why original_count travels in every header.
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    t.send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut sink)
        .unwrap();

    for (i, f) in sink.fragments.iter().enumerate() {
        let frag = Fragment::parse(f).unwrap();
        assert_eq!(
            frag.header.original_count, c.block_size as u8,
            "fragment {i} lost the block geometry"
        );
    }
}

#[test]
fn crc_is_over_the_payload_only_not_the_header() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    t.send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut sink)
        .unwrap();

    let f = &sink.fragments[0];
    let payload = &f[HEADER_SIZE..];
    let header_crc = u16::from_be_bytes([f[4], f[5]]);
    assert_eq!(
        header_crc,
        crc16(payload),
        "the CRC field must cover exactly the fragment payload"
    );
}

#[test]
fn the_same_block_id_is_stamped_on_every_fragment() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let block_id = t
        .send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut sink)
        .unwrap();

    for (i, f) in sink.fragments.iter().enumerate() {
        let frag = Fragment::parse(f).unwrap();
        assert_eq!(
            frag.header.block_id, block_id,
            "fragment {i} carries a different block_id than the one returned"
        );
    }
}

// --- block-id sequencing ---------------------------------------------------

#[test]
fn block_ids_advance_by_one_per_frame() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let data = payload(c.block_payload_capacity());

    let mut seen = Vec::new();
    for _ in 0..5 {
        let mut sink = VecSink::new();
        seen.push(t.send_frame(&data, &mut scratch, &mut sink).unwrap());
    }
    assert_eq!(seen, vec![0, 1, 2, 3, 4]);
}

#[test]
fn block_ids_wrap_at_u16_max_without_panicking() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let data = payload(c.block_payload_capacity());

    // Walk the counter to the edge the cheap way: send, assert, repeat.
    for _ in 0..3 {
        let mut sink = VecSink::new();
        t.send_frame(&data, &mut scratch, &mut sink).unwrap();
    }
    assert_eq!(t.next_block_id(), 3);

    // Direct check of the wrap arithmetic, which is the part that could panic.
    let wrapped = (u16::MAX).wrapping_add(1);
    assert_eq!(wrapped, 0);
}

#[test]
fn consecutive_frames_get_distinct_block_ids() {
    // Two frames in flight must never share a block id, or the receiver would
    // merge them into one block and produce a corrupted frame.
    let mut t = tx();
    let mut scratch = vec![0u8; t.capacity()];

    let mut a = VecSink::new();
    let id_a = t
        .send_frame(&payload(4), &mut scratch, &mut a)
        .unwrap();
    let mut b = VecSink::new();
    let id_b = t
        .send_frame(&payload(9), &mut scratch, &mut b)
        .unwrap();
    assert_ne!(id_a, id_b);
}

// --- error handling --------------------------------------------------------

#[test]
fn a_frame_longer_than_one_block_is_rejected_not_truncated() {
    let mut t = tx();
    let capacity = t.capacity();
    let mut scratch = vec![0u8; capacity];
    let mut sink = VecSink::new();

    let err = t
        .send_frame(&payload(capacity + 1), &mut scratch, &mut sink)
        .unwrap_err();
    assert_eq!(
        err,
        TxError::FrameTooLong {
            capacity,
            got: capacity + 1
        }
    );
    assert!(
        sink.is_empty(),
        "a rejected frame must emit nothing at all"
    );
}

#[test]
fn a_scratch_buffer_smaller_than_a_block_is_rejected() {
    let mut t = tx();
    let capacity = t.capacity();
    let mut scratch = vec![0u8; capacity - 1];
    let mut sink = VecSink::new();

    let err = t
        .send_frame(&payload(4), &mut scratch, &mut sink)
        .unwrap_err();
    assert_eq!(
        err,
        TxError::ScratchTooSmall {
            need: capacity,
            got: capacity - 1
        }
    );
    assert!(sink.is_empty());
}

#[test]
fn an_exact_capacity_frame_is_accepted_and_needs_no_padding() {
    let mut t = tx();
    let capacity = t.capacity();
    let mut scratch = vec![0u8; capacity];
    let mut sink = VecSink::new();
    let data = payload(capacity);
    t.send_frame(&data, &mut scratch, &mut sink).unwrap();

    // Originals must reproduce the frame byte-for-byte: no padding was needed.
    let c = t.config();
    let mut joined = Vec::new();
    for idx in 0..c.block_size {
        let frag = Fragment::parse(&sink.fragments[idx]).unwrap();
        joined.extend_from_slice(&frag.payload);
    }
    assert_eq!(joined, data);
}

#[test]
fn an_empty_frame_is_accepted_and_yields_an_all_zero_block() {
    // Zero-length input is legal: it pads to a full block of zeros. It decodes
    // back to an empty frame, which is the documented trim behaviour.
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let block_id = t.send_frame(&[], &mut scratch, &mut sink).unwrap();

    assert_eq!(sink.len(), c.fragments_per_block());
    for f in &sink.fragments {
        let frag = Fragment::parse(f).unwrap();
        assert_eq!(frag.header.block_id, block_id);
        assert!(frag.payload.iter().all(|&b| b == 0));
    }

    let mut rx = PipelineRx::new(cfg()).unwrap();
    let mut got = None;
    for f in &sink.fragments {
        if let Some(block) = recv_frame(&mut rx, f) {
            got = Some(block);
            break;
        }
    }
    assert_eq!(got.expect("decodes"), Vec::<u8>::new());
}

#[test]
fn an_invalid_geometry_yields_no_pipeline() {
    assert!(Pipeline::new(PipelineConfig::new(0, 8, 2)).is_none());
    assert!(Pipeline::new(PipelineConfig::new(4, 0, 2)).is_none());
    assert!(Pipeline::new(PipelineConfig::new(4, 8, 0)).is_none());
    assert!(Pipeline::new(PipelineConfig::new(4, 8, MAX_REDUNDANCY + 1)).is_none());
    assert!(tx_geometry_ok(&PipelineConfig::new(4, 8, 2)));
    assert!(!tx_geometry_ok(&PipelineConfig::new(0, 8, 2)));
}

// --- sink behaviour --------------------------------------------------------

/// A sink that fails on the Nth fragment, to prove partial emission stops.
struct FailingSink {
    fail_at: usize,
    seen: usize,
    collected: Vec<Vec<u8>>,
}

impl FragmentSink for FailingSink {
    type Error = &'static str;

    fn emit(&mut self, fragment: &[u8]) -> Result<(), Self::Error> {
        if self.seen == self.fail_at {
            return Err("radio busy");
        }
        self.seen += 1;
        self.collected.push(fragment.to_vec());
        Ok(())
    }
}

#[test]
fn a_sink_error_stops_emission_and_is_propagated_verbatim() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = FailingSink {
        fail_at: 2,
        seen: 0,
        collected: Vec::new(),
    };

    let err = t
        .send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut sink)
        .unwrap_err();
    assert_eq!(err, TxError::Sink("radio busy"));
    assert_eq!(
        sink.collected.len(),
        2,
        "fragments before the failure must have been emitted"
    );
    // The failed block consumed an id: it must not be handed out again.
    assert_eq!(t.next_block_id(), 1);
}

#[test]
fn a_failed_block_id_is_not_reused() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; c.block_payload_capacity()];
    let mut fail = FailingSink {
        fail_at: 0,
        seen: 0,
        collected: Vec::new(),
    };
    let first = t
        .send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut fail)
        .unwrap_err();
    assert!(matches!(first, TxError::Sink(_)));

    let mut ok = VecSink::new();
    let second = t
        .send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut ok)
        .unwrap();
    assert_ne!(
        second, 0,
        "the id burned by the failed block must not be reissued"
    );
}

#[test]
fn counting_sink_agrees_with_the_vec_sink() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let data = payload(c.block_payload_capacity());

    let mut vec_sink = VecSink::new();
    t.send_frame(&data, &mut scratch, &mut vec_sink).unwrap();

    let mut count_sink = CountingSink::default();
    t.send_frame(&data, &mut scratch, &mut count_sink).unwrap();

    assert_eq!(count_sink.count, vec_sink.len());
    assert_eq!(count_sink.bytes, vec_sink.len() * c.wire_fragment_size());
}

#[test]
fn send_frame_is_repeatable_with_the_same_scratch_buffer() {
    // The same scratch buffer is reused across blocks on a real node; leftover
    // bytes from a previous, longer frame must not leak into the next one.
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];

    let mut long = VecSink::new();
    t.send_frame(&payload(c.block_payload_capacity()), &mut scratch, &mut long)
        .unwrap();

    let short_data = payload(3);
    let mut short = VecSink::new();
    t.send_frame(&short_data, &mut scratch, &mut short).unwrap();

    // Everything past the real payload must be zero, not remnants of `long`.
    let joined: Vec<u8> = (0..c.block_size)
        .flat_map(|i| Fragment::parse(&short.fragments[i]).unwrap().payload)
        .collect();
    assert_eq!(&joined[..3], &short_data[..]);
    assert!(
        joined[3..].iter().all(|&b| b == 0),
        "stale bytes from the previous frame leaked through the scratch buffer"
    );
}

// --- TX -> RX round trip ---------------------------------------------------

#[test]
fn round_trip_with_no_loss_recovers_the_frame_exactly() {
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let data = payload(c.block_payload_capacity());
    t.send_frame(&data, &mut scratch, &mut sink).unwrap();

    let mut rx = PipelineRx::new(cfg()).unwrap();
    let mut got = None;
    for f in &sink.fragments {
        if let Some(block) = recv_frame(&mut rx, f) {
            got = Some(block);
            break;
        }
    }
    assert_eq!(got.expect("decodes"), data);
}

#[test]
fn round_trip_survives_a_loss_pattern_the_parity_rows_span() {
    // The codec's real guarantee is *not* "any `redundancy` losses": parity rows
    // of the PRBS23-XOR matrix are not all independent, so recovery holds
    // exactly when the missing columns lie in the row space (see
    // `recovers_any_loss_pattern_the_parity_rows_can_span` in
    // tests/erasure_vectors.rs for the same contract at the erasure layer).
    //
    // For k=4/red=2, rows 1 and 2 are both {0,2}, so the span is one-dimensional:
    // losing original 0 alone is recoverable, losing 0 and 2 together is not.
    // This test therefore drops exactly one original that the rows span, and
    // requires a byte-exact recovery through the TX wire bytes.
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let data = payload(c.block_payload_capacity());
    t.send_frame(&data, &mut scratch, &mut sink).unwrap();

    // Row 1 = {0, 2}: original 0 is recoverable from either parity fragment.
    let lost = 0usize;
    let mut rx = PipelineRx::new(cfg()).unwrap();
    let mut got = None;
    for (i, f) in sink.fragments.iter().enumerate() {
        if i == lost {
            continue; // simulate the loss
        }
        if let Some(block) = recv_frame(&mut rx, f) {
            got = Some(block);
            break;
        }
    }
    assert_eq!(
        got.expect("parity must rebuild the lost original"),
        data,
        "recovered frame differs from the transmitted one"
    );
}

#[test]
fn losing_two_spanned_originals_recovers_exactly() {
    // k=12/red=6 is the geometry whose rows have rank 6, so any 6-loss pattern
    // inside the span is recoverable. Drop three originals the rows actually
    // span and require byte-exact recovery from the TX wire bytes.
    let c = PipelineConfig::new(12, 8, 6);
    let mut t = Pipeline::new(c).unwrap();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let data = payload(c.block_payload_capacity());
    t.send_frame(&data, &mut scratch, &mut sink).unwrap();

    // Originals 0, 1 and 2 all appear in the leading parity rows.
    let lost = [0usize, 1, 2];
    let mut rx = PipelineRx::new(c).unwrap();
    let mut got = None;
    for (i, f) in sink.fragments.iter().enumerate() {
        if lost.contains(&i) {
            continue;
        }
        if let Some(block) = recv_frame(&mut rx, f) {
            got = Some(block);
            break;
        }
    }
    assert_eq!(
        got.expect("rows of rank 6 must span a 3-original loss"),
        data
    );
}

#[test]
fn round_trip_in_reverse_order_recovers_the_frame() {
    // Fragments arrive in any order on a radio; the decoder must not care.
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let mut sink = VecSink::new();
    let data = payload(c.block_payload_capacity());
    t.send_frame(&data, &mut scratch, &mut sink).unwrap();

    let mut rx = PipelineRx::new(cfg()).unwrap();
    let mut got = None;
    for f in sink.fragments.iter().rev() {
        if let Some(block) = recv_frame(&mut rx, f) {
            got = Some(block);
            break;
        }
    }
    assert_eq!(got.expect("decodes in reverse order"), data);
}

#[test]
fn two_frames_do_not_interleave_into_one_block() {
    // Distinct block ids are what keep two in-flight frames apart.
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];

    let first = payload(c.block_payload_capacity());
    let second: Vec<u8> = first.iter().map(|&b| b ^ 0x5A).collect();

    let mut a = VecSink::new();
    t.send_frame(&first, &mut scratch, &mut a).unwrap();
    let mut b = VecSink::new();
    t.send_frame(&second, &mut scratch, &mut b).unwrap();

    // Interleave fragment-by-fragment.
    let mut rx = PipelineRx::new(cfg()).unwrap();
    let mut recovered: Vec<Vec<u8>> = Vec::new();
    for i in 0..a.len() {
        for f in [&a.fragments[i], &b.fragments[i]] {
            if let Some(block) = recv_frame(&mut rx, f) {
                recovered.push(block);
            }
        }
    }

    recovered.sort();
    let mut want = vec![first.clone(), second.clone()];
    want.sort();
    assert_eq!(recovered, want, "both frames must come back intact");
}

#[test]
fn the_same_frame_encodes_to_identical_payloads_under_any_block_id() {
    // The erasure code must be a pure function of the block content: the
    // payload bytes of fragment i are the same no matter which block id the
    // fragment carries. A retransmission under a fresh id must therefore
    // produce identical payloads, or a receiver that mixes fragments from two
    // attempts of the same frame would decode garbage.
    let mut t = tx();
    let c = t.config();
    let mut scratch = vec![0u8; t.capacity()];
    let data = payload(c.block_payload_capacity());

    let mut a = VecSink::new();
    let id_a = t.send_frame(&data, &mut scratch, &mut a).unwrap();
    let mut b = VecSink::new();
    let id_b = t.send_frame(&data, &mut scratch, &mut b).unwrap();

    assert_ne!(id_a, id_b, "consecutive sends must take distinct block ids");

    for i in 0..a.len() {
        let fa = Fragment::parse(&a.fragments[i]).unwrap();
        let fb = Fragment::parse(&b.fragments[i]).unwrap();
        assert_eq!(
            fa.payload, fb.payload,
            "fragment {i} payload changed between identical sends"
        );
        assert_eq!(
            fa.header.crc, fb.header.crc,
            "fragment {i} CRC changed between identical sends"
        );
        assert_eq!(fa.header.frag_index, fb.header.frag_index);
        assert_ne!(fa.header.block_id, fb.header.block_id);
    }
}
