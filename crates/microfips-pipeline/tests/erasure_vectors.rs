//! Erasure-code port fidelity tests.
//!
//! These pin the PRBS23-XOR erasure codec to the behaviour of the
//! balloon-fresh C original (`tracker/firmware/components/erasure/erasure.c`),
//! which is the reference implementation. Recovery properties are asserted
//! statistically across many random loss patterns rather than against a single
//! hand-picked case, because the parity matrix is pseudo-random.

use microfips_pipeline::erasure::{
    prbs23, DecodeStatus, ErasureDecoder, ErasureEncoder, MAX_FRAGS, MAX_FRAG_SIZE, MAX_REDUNDANCY,
};

/// Deterministic xorshift PRNG so a failure is reproducible from the seed.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// Build a deterministic pseudo-random block of `k * frag_size` bytes.
fn make_block(k: usize, frag_size: usize, seed: u64) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    (0..k * frag_size).map(|_| (rng.next_u64() & 0xFF) as u8).collect()
}

/// Encode all `redundancy` parity fragments for a block.
fn encode_parity(block: &[u8], k: usize, frag_size: usize, redundancy: usize) -> Vec<Vec<u8>> {
    let enc = ErasureEncoder::new(k, frag_size, block).expect("encoder");
    (0..redundancy)
        .map(|r| {
            let mut out = vec![0u8; frag_size];
            assert!(enc.encode_redundant(r, &mut out), "encode r={r}");
            out
        })
        .collect()
}

/// Fresh decoder scratch buffers sized for `k`/`frag_size`.
fn decoder<'a>(k: usize, frag_size: usize, assembly: &'a mut Vec<u8>, matrix: &'a mut Vec<u8>) -> ErasureDecoder<'a> {
    assembly.resize(k * frag_size + MAX_REDUNDANCY * frag_size, 0);
    matrix.resize(MAX_REDUNDANCY * ((k >> 3) + 1), 0);
    ErasureDecoder::new(k, frag_size, assembly, matrix).expect("decoder")
}

// ---------------------------------------------------------------------------
// PRBS23 LFSR — the heart of the parity matrix.
// ---------------------------------------------------------------------------

#[test]
fn prbs23_matches_c_reference() {
    // Hand-computed against the C `prbs23()`:
    //   prbs23(1) = (1>>1) + (((1&1) ^ ((1&0x20)>>5)) << 22) = 0 + 1<<22
    assert_eq!(prbs23(1), 1 << 22);
    // b0 = 0, b1 = 0 -> 0x400000 >> 1 = 0x200000
    assert_eq!(prbs23(1 << 22), 1 << 21);
    // A value with bit0 set and bit5 clear flips the feedback bit high.
    assert_eq!(prbs23(0b100001), {
        let b0 = 1;
        let b1 = (0b100001 & 0x20) >> 5; // = 1
        (0b100001 >> 1) + ((b0 ^ b1) << 22) // = 0b10000 + 0
    });
    // Bit0 and bit5 both set -> feedback 0.
    assert_eq!(prbs23(0b100001 | 0x20), {
        let v = 0b100001 | 0x20;
        let b0 = v & 1;
        let b1 = (v & 0x20) >> 5;
        (v >> 1) + ((b0 ^ b1) << 22)
    });
}

#[test]
fn prbs23_stays_in_23_bit_range() {
    // The C code treats the value as a 23-bit register. 10000 steps of the
    // recurrence must never escape [0, 2^23).
    let mut x = 1i32;
    for _ in 0..10_000 {
        x = prbs23(x);
        assert!(x >= 0, "went negative: {x}");
        assert!(x < (1 << 23), "escaped 23 bits: {x}");
    }
}

// ---------------------------------------------------------------------------
// Encoder/decoder behaviour.
// ---------------------------------------------------------------------------

#[test]
fn all_originals_present_decodes_without_recovery() {
    let (k, frag_size) = (8usize, 32usize);
    let block = make_block(k, frag_size, 42);

    let mut assembly = Vec::new();
    let mut matrix = Vec::new();
    let mut dec = decoder(k, frag_size, &mut assembly, &mut matrix);

    for i in 0..k {
        let off = i * frag_size;
        let status = dec.process((i + 1) as u16, &block[off..off + frag_size]);
        if i + 1 < k {
            assert_eq!(status, DecodeStatus::Progress, "frag {i}");
        } else {
            assert_eq!(status, DecodeStatus::Complete, "last frag");
        }
    }

    assert!(dec.is_complete());
    assert_eq!(dec.assembled().unwrap(), &block[..]);
}

#[test]
fn one_missing_original_is_recovered_from_parity() {
    let (k, frag_size) = (10usize, 64usize);
    let block = make_block(k, frag_size, 7);
    let parity = encode_parity(&block, k, frag_size, MAX_REDUNDANCY);

    // Drop original 4 of 10.
    let lost = 4usize;
    let mut assembly = Vec::new();
    let mut matrix = Vec::new();
    let mut dec = decoder(k, frag_size, &mut assembly, &mut matrix);

    for i in 0..k {
        if i == lost {
            continue;
        }
        let off = i * frag_size;
        dec.process((i + 1) as u16, &block[off..off + frag_size]);
    }
    assert!(!dec.is_complete(), "one original still missing");

    // Feed parity rows until the decoder solves it.
    let mut solved = false;
    for (r, p) in parity.iter().enumerate() {
        let counter = (k + r + 1) as u16;
        if dec.process(counter, p) == DecodeStatus::Complete {
            solved = true;
            break;
        }
    }

    assert!(solved, "decoder never completed");
    assert_eq!(dec.assembled().unwrap(), &block[..]);
    assert_eq!(
        dec.original(lost).unwrap(),
        &block[lost * frag_size..(lost + 1) * frag_size]
    );
}

#[test]
fn recovers_from_every_single_fragment_loss() {
    let (k, frag_size, redundancy) = (8usize, 32usize, 4usize);
    let block = make_block(k, frag_size, 99);
    let parity = encode_parity(&block, k, frag_size, redundancy);
    let total = k + redundancy;

    // Drop each fragment index in turn; the block must still reconstruct.
    for lost in 0..total {
        let mut assembly = Vec::new();
        let mut matrix = Vec::new();
        let mut dec = decoder(k, frag_size, &mut assembly, &mut matrix);

        for counter in 1..=total {
            if counter - 1 == lost {
                continue;
            }
            let payload: &[u8] = if counter <= k {
                let off = (counter - 1) * frag_size;
                &block[off..off + frag_size]
            } else {
                &parity[counter - k - 1]
            };
            dec.process(counter as u16, payload);
        }

        assert!(dec.is_complete(), "loss of frag {lost} not recovered");
        assert_eq!(dec.assembled().unwrap(), &block[..], "loss of frag {lost}");
    }
}

#[test]
fn recovers_up_to_redundancy_losses_over_random_patterns() {
    // The headline property: any `redundancy` losses out of `k + redundancy`
    // are recoverable, regardless of which ones. Property-tested over many
    // random patterns.
    let (k, frag_size, redundancy) = (12usize, 48usize, 6usize);
    let total = k + redundancy;
    let mut rng = Rng::new(0xC0FFEE);

    for trial in 0..200 {
        let block = make_block(k, frag_size, 1000 + trial as u64);
        let parity = encode_parity(&block, k, frag_size, redundancy);

        // Choose exactly `redundancy` distinct fragments to drop.
        let mut order: Vec<usize> = (0..total).collect();
        for i in (1..order.len()).rev() {
            let j = rng.below(i + 1);
            order.swap(i, j);
        }
        let lost: Vec<bool> = {
            let mut v = vec![false; total];
            for &idx in order.iter().take(redundancy) {
                v[idx] = true;
            }
            v
        };

        let mut assembly = Vec::new();
        let mut matrix = Vec::new();
        let mut dec = decoder(k, frag_size, &mut assembly, &mut matrix);

        // Deliver surviving fragments in the shuffled order.
        for &idx in order.iter() {
            if lost[idx] {
                continue;
            }
            let counter = idx + 1;
            let payload: &[u8] = if counter <= k {
                let off = idx * frag_size;
                &block[off..off + frag_size]
            } else {
                &parity[counter - k - 1]
            };
            dec.process(counter as u16, payload);
        }

        assert!(
            dec.is_complete(),
            "trial {trial}: failed to recover with {} losses ({:?})",
            redundancy,
            (0..total).filter(|i| lost[*i]).collect::<Vec<_>>()
        );
        assert_eq!(
            dec.assembled().unwrap(),
            &block[..],
            "trial {trial}: reconstructed bytes differ"
        );
    }
}

#[test]
fn fails_gracefully_when_losses_exceed_redundancy() {
    // Lose (redundancy + 1) fragments: reconstruction must not happen, and
    // must not panic or report garbage as complete.
    let (k, frag_size, redundancy) = (10usize, 32usize, 3usize);
    let block = make_block(k, frag_size, 5);
    let parity = encode_parity(&block, k, frag_size, redundancy);
    let total = k + redundancy;

    // Drop originals 0..=redundancy (redundancy + 1 originals lost).
    let lost_set: Vec<usize> = (0..=redundancy).collect();

    let mut assembly = Vec::new();
    let mut matrix = Vec::new();
    let mut dec = decoder(k, frag_size, &mut assembly, &mut matrix);

    for idx in 0..total {
        if lost_set.contains(&idx) {
            continue;
        }
        let counter = idx + 1;
        let payload: &[u8] = if counter <= k {
            let off = idx * frag_size;
            &block[off..off + frag_size]
        } else {
            &parity[counter - k - 1]
        };
        dec.process(counter as u16, payload);
    }

    assert!(
        !dec.is_complete(),
        "decoder claimed success with {} losses and only {} parity fragments",
        lost_set.len(),
        redundancy
    );
    assert!(dec.assembled().is_none());
}

#[test]
fn out_of_order_delivery_decodes_identically() {
    let (k, frag_size, redundancy) = (8usize, 32usize, 4usize);
    let block = make_block(k, frag_size, 2024);
    let parity = encode_parity(&block, k, frag_size, redundancy);
    let total = k + redundancy;

    // Reverse order, drop 3 originals.
    let lost = [1usize, 3, 6];
    let mut assembly = Vec::new();
    let mut matrix = Vec::new();
    let mut dec = decoder(k, frag_size, &mut assembly, &mut matrix);

    for idx in (0..total).rev() {
        if lost.contains(&idx) {
            continue;
        }
        let counter = idx + 1;
        let payload: &[u8] = if counter <= k {
            let off = idx * frag_size;
            &block[off..off + frag_size]
        } else {
            &parity[counter - k - 1]
        };
        dec.process(counter as u16, payload);
    }

    assert!(dec.is_complete(), "reverse-order delivery failed");
    assert_eq!(dec.assembled().unwrap(), &block[..]);
}

#[test]
fn invalid_fragments_are_rejected_not_panicked() {
    let (k, frag_size) = (4usize, 16usize);
    let block = make_block(k, frag_size, 3);
    let mut assembly = Vec::new();
    let mut matrix = Vec::new();
    let mut dec = decoder(k, frag_size, &mut assembly, &mut matrix);

    // Counter 0 (1-based counters start at 1).
    assert_eq!(dec.process(0, &block[..frag_size]), DecodeStatus::Invalid);
    // Counter beyond k + MAX_REDUNDANCY.
    assert_eq!(
        dec.process((k + MAX_REDUNDANCY + 1) as u16, &block[..frag_size]),
        DecodeStatus::Invalid
    );
    // Short payload.
    assert_eq!(dec.process(1, &block[..frag_size - 1]), DecodeStatus::Invalid);
}

#[test]
fn geometry_limits_are_enforced() {
    let block = make_block(4, 16, 1);
    // Too many fragments.
    assert!(ErasureEncoder::new(MAX_FRAGS + 1, 16, &block).is_none());
    // Fragment too large.
    assert!(ErasureEncoder::new(4, MAX_FRAG_SIZE + 1, &block).is_none());
    // Zero geometry.
    assert!(ErasureEncoder::new(0, 16, &block).is_none());
    assert!(ErasureEncoder::new(4, 0, &block).is_none());
    // Buffer too short.
    assert!(ErasureEncoder::new(4, 16, &block[..10]).is_none());
    // Valid case for contrast.
    assert!(ErasureEncoder::new(4, 16, &block).is_some());

    let mut assembly = vec![0u8; 4 * 16 + MAX_REDUNDANCY * 16];
    let mut matrix = vec![0u8; MAX_REDUNDANCY];
    assert!(ErasureDecoder::new(MAX_FRAGS + 1, 16, &mut assembly, &mut matrix).is_none());
    // Undersized scratch buffers rejected.
    let mut small_assembly = vec![0u8; 4];
    let mut small_matrix = vec![0u8; 4];
    assert!(ErasureDecoder::new(4, 16, &mut small_assembly, &mut small_matrix).is_none());
}

#[test]
fn encode_redundant_rejects_bad_index_and_short_buffer() {
    let (k, frag_size) = (4usize, 16usize);
    let block = make_block(k, frag_size, 11);
    let enc = ErasureEncoder::new(k, frag_size, &block).unwrap();

    let mut out = vec![0u8; frag_size];
    assert!(enc.encode_redundant(0, &mut out));
    assert!(!enc.encode_redundant(MAX_REDUNDANCY, &mut out));
    let mut short = vec![0u8; frag_size - 1];
    assert!(!enc.encode_redundant(0, &mut short));
}

#[test]
fn completing_twice_is_idempotent() {
    // Once complete, further fragments are absorbed harmlessly; the assembled
    // buffer must not be disturbed.
    let (k, frag_size) = (4usize, 16usize);
    let block = make_block(k, frag_size, 77);
    let mut assembly = Vec::new();
    let mut matrix = Vec::new();
    let mut dec = decoder(k, frag_size, &mut assembly, &mut matrix);

    for i in 0..k {
        let off = i * frag_size;
        dec.process((i + 1) as u16, &block[off..off + frag_size]);
    }
    assert!(dec.is_complete());
    let snapshot = dec.assembled().unwrap().to_vec();

    // A stale fragment arriving late.
    let bogus = vec![0xFFu8; frag_size];
    assert_eq!(dec.process(1, &bogus), DecodeStatus::Complete);
    assert_eq!(dec.assembled().unwrap(), &snapshot[..]);
}

#[test]
fn maximum_geometry_round_trips() {
    // The biggest block the codec supports, with maximum redundancy.
    let (k, frag_size) = (MAX_FRAGS, 32usize);
    let redundancy = MAX_REDUNDANCY;
    let block = make_block(k, frag_size, 0xBEEF);
    let parity = encode_parity(&block, k, frag_size, redundancy);
    let total = k + redundancy;

    // Lose the first `redundancy` originals outright.
    let mut assembly = Vec::new();
    let mut matrix = Vec::new();
    let mut dec = decoder(k, frag_size, &mut assembly, &mut matrix);

    for idx in 0..total {
        if idx < redundancy {
            continue;
        }
        let counter = idx + 1;
        let payload: &[u8] = if counter <= k {
            let off = idx * frag_size;
            &block[off..off + frag_size]
        } else {
            &parity[counter - k - 1]
        };
        dec.process(counter as u16, payload);
    }

    assert!(dec.is_complete(), "max geometry failed to recover");
    assert_eq!(dec.assembled().unwrap(), &block[..]);
}

#[test]
fn parity_is_xor_of_selected_originals() {
    // Verify the encoder's contract directly: a parity fragment equals the XOR
    // of exactly the originals its matrix row covers. We re-derive the row with
    // the same PRBS23 stream the C code uses and check bit-count is k/2.
    let (k, frag_size) = (8usize, 32usize);
    let block = make_block(k, frag_size, 1234);
    let enc = ErasureEncoder::new(k, frag_size, &block).unwrap();

    let mut out = vec![0u8; frag_size];
    assert!(enc.encode_redundant(0, &mut out));

    // Independently recompute row 1's coverage via the public PRBS23.
    let m = k as i32;
    let m_temp = if (m & (m - 1)) == 0 { 1 } else { 0 };
    let mut x = 1 + 1001 * 1;
    let mut covered = vec![false; k];
    let mut nb_coeff = 0;
    while nb_coeff < (m >> 1) {
        let mut r = 1 << 16;
        while r >= m {
            x = prbs23(x);
            r = x % (m + m_temp);
        }
        covered[r as usize] = true;
        nb_coeff += 1;
    }
    assert_eq!(
        covered.iter().filter(|c| **c).count(),
        k / 2,
        "row must set exactly k/2 bits"
    );

    let mut expected = vec![0u8; frag_size];
    for (i, c) in covered.iter().enumerate() {
        if *c {
            let off = i * frag_size;
            for b in 0..frag_size {
                expected[b] ^= block[off + b];
            }
        }
    }
    assert_eq!(out, expected, "encoder does not match the documented matrix");
}
