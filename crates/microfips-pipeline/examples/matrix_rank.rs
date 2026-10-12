//! Diagnostic: measure the real bit-weight and linear independence of the
//! parity matrix rows produced by the PRBS23-XOR codec.
//!
//! This exists because the codec's *nominal* parameters (m/2 bits per row,
//! `redundancy` rows) do not describe its *effective* strength: the row
//! generator loops a fixed number of LFSR draws and ORs each draw's column into
//! the row, so repeated draws collapse and rows come out lighter -- and more
//! dependent -- than intended. Those row weights are what bound recoverable
//! loss, so the RX design has to be sized from them, not from the nominal
//! numbers.

use microfips_pipeline::erasure::{parity_row_bits, MAX_FRAGS};

fn rank(rows: &[Vec<usize>], ncols: usize) -> usize {
    let mut basis: Vec<u128> = Vec::new();
    for row in rows {
        let mut x: u128 = 0;
        for &c in row {
            x |= 1u128 << c;
        }
        for &b in &basis {
            x = x.min(x ^ b);
        }
        if x != 0 {
            basis.push(x);
        }
    }
    let _ = ncols;
    basis.len()
}

fn main() {
    for m in [8usize, 12, 16, 24, 32] {
        let half = m / 2;
        let mut rows = Vec::new();
        let mut weights = Vec::new();
        for n in 1..=half {
            let bits = parity_row_bits(n as i32, m as i32);
            weights.push(bits.len());
            rows.push(bits);
        }
        let r = rank(&rows, m);
        println!(
            "m={m:3} nominal_half={half:2} rows={} weights={weights:?} rank={r} (need {m} for full recovery)",
            rows.len()
        );
    }
    assert!(MAX_FRAGS >= 64);
}
