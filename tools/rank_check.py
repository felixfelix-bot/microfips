#!/usr/bin/env python3
"""Compute the true GF(2) rank of the PRBS23 parity matrix and the achievable
recovery rate, independently of the Rust implementation.

This mirrors get_parity_matrix_row() from
~/repos/balloon-fresh/tracker/firmware/components/erasure/erasure.c exactly so
the numbers quoted in PROGRESS/REPORT can be cross-checked against a second
implementation.
"""

MAX_FRAGS = 64


def prbs23(v: int) -> int:
    v &= (1 << 23) - 1
    b0 = v & 0x01
    b1 = (v & 0x20) >> 5
    return (v >> 1) + ((b0 ^ b1) << 22)


def parity_row(n: int, m: int) -> int:
    """Row `n` as a bitmask over m columns (bit i = column i)."""
    m_temp = 1 if (m & (m - 1)) == 0 else 0
    x = 1 + (1001 * n)
    row = 0
    nb_coeff = 0
    while nb_coeff < (m >> 1):
        r = 1 << 16
        while r >= m:
            x = prbs23(x)
            r = x % (m + m_temp)
        row |= 1 << r
        nb_coeff += 1
    return row


def rank(rows, nbits=128):
    basis = []
    for row in rows:
        x = row
        for b in basis:
            x = min(x, x ^ b)
        if x:
            basis.append(x)
    return len(basis)


def spanable(parity_rows_present, lost_cols):
    """Full rank on the projection onto lost_cols?"""
    if not lost_cols:
        return True
    rows = []
    for row in parity_rows_present:
        proj = 0
        for idx, col in enumerate(lost_cols):
            if (row >> col) & 1:
                proj |= 1 << idx
        rows.append(proj)
    return rank(rows) == len(lost_cols)


def main():
    print("true rank of the parity rows (rows 1..redundancy):")
    for m in (8, 12, 16, 24, 32, 64):
        red = m // 2
        rows = [parity_row(n, m) for n in range(1, red + 1)]
        weights = [bin(r).count("1") for r in rows]
        print(
            f"  m={m:3} redundancy={red:2} rank={rank(rows):3} (need {m:3})"
            f" weights={weights}"
        )

    print()
    print("single-loss recoverability (rank of one projected row >= 1):")
    for m in (8, 12, 16, 24, 32):
        red = m // 2
        rows = [parity_row(n, m) for n in range(1, red + 1)]
        ok = sum(
            1 for c in range(m) if spanable(rows, [c])
        )
        print(f"  m={m:3}: {ok}/{m} single-column losses spanable")


if __name__ == "__main__":
    main()
