//! Dump the exact bits the decoder stores for each parity row, to compare
//! against the shared generator's rows.
use microfips_pipeline::erasure::{parity_row_bits, ErasureDecoder, ErasureEncoder};

fn main() {
    let (k, frag_size, redundancy) = (12usize, 48usize, 6usize);
    let block = vec![0xAAu8; k * frag_size];

    let enc = ErasureEncoder::new(k, frag_size, &block).unwrap();
    let mut parity = vec![0u8; redundancy * frag_size];
    for r in 0..redundancy {
        enc.encode_redundant(r, &mut parity[r * frag_size..(r + 1) * frag_size]);
    }

    let row_bytes = (k >> 3) + 1;
    let mut assembly = vec![0u8; k * frag_size + 32 * frag_size];
    let mut matrix = vec![0u8; 32 * row_bytes];
    let mut dec = ErasureDecoder::new(k, frag_size, &mut assembly, &mut matrix).unwrap();

    // Feed parity rows 0..3 only (r=0..3 -> counter 13..16).
    for r in 0..4 {
        dec.process((k + 1 + r) as u16, &parity[r * frag_size..(r + 1) * frag_size]);
    }

    // Read back the matrix the decoder wrote (it is our buffer).
    for row in 0..4 {
        let off = row * row_bytes;
        let mut bits = Vec::new();
        for i in 0..k {
            if (matrix[off + (i >> 3)] >> (i & 7)) & 1 == 1 {
                bits.push(i);
            }
        }
        println!(
            "stored row {row} (raw bytes {:?}) bits={bits:?}  expected={:?}",
            &matrix[off..off + row_bytes],
            parity_row_bits((row + 1) as i32, k as i32)
        );
    }
}
