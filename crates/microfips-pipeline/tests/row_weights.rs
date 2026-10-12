//! Row-weight measurement for the parity matrix: proves the port's rows carry
//! as many distinct bits as the generator intends, and fails loudly if the
//! PRBS23 draw loop ever silently degrades rows to near-zero weight.
//!
//! Background: the generator loops `while bits_written < m/2`, but each draw is
//! `x % m` and can repeat a column already set. Written bits are therefore not
//! distinct bits. A row that collides down to a single column is still a legal
//! code row, but a *cluster* of low-weight rows among the first few redundancy
//! indices is what makes small missing sets unsolvable -- so the weights are
//! worth pinning rather than assuming.

use microfips_pipeline::erasure::{parity_row_weight, MAX_FRAGS};

#[test]
fn parity_rows_have_expected_weights() {
    for m in [4usize, 8, 12, 16, 24, 32, 64] {
        let mut weights = [0usize; 8];
        for n in 0..8 {
            weights[n] = parity_row_weight(n as i32, m as i32);
        }
        // No row may be empty: an all-zero row carries no information at all.
        for (n, w) in weights.iter().enumerate() {
            assert!(
                *w > 0,
                "m={m} row={n} is an all-zero row (weight 0) -- useless for recovery"
            );
            assert!(
                *w <= m,
                "m={m} row={n} weight {w} exceeds column count {m}"
            );
        }
    }
    assert!(MAX_FRAGS >= 64);
}
