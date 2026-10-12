//! Compare the parity rows the DECODER stores against the rows the shared
//! generator produces. A mismatch here means the decoder indexes the generator
//! differently from the encoder, which silently breaks recovery for every
//! fragment the encoder produced.
use microfips_pipeline::erasure::{parity_row_bits, DecodeStatus, ErasureDecoder, ErasureEncoder};

fn main() {
    let (k, frag_size, redundancy) = (12usize, 48usize, 6usize);
    let block = vec![0xAAu8; k * frag_size];

    let enc = ErasureEncoder::new(k, frag_size, &block).unwrap();
    let mut parity = vec![0u8; redundancy * frag_size];
    for r in 0..redundancy {
        enc.encode_redundant(r, &mut parity[r * frag_size..(r + 1) * frag_size]);
    }

    // What the generator says each encoder index maps to.
    for r in 0..redundancy {
        println!(
            "encoder r={r} uses generator row {} -> bits {:?}",
            r + 1,
            parity_row_bits((r + 1) as i32, k as i32)
        );
    }

    // What the decoder stores: feed parity fragment `counter = k+1+r` first.
    for r in 0..redundancy {
        let mut assembly = vec![0u8; k * frag_size + 32 * frag_size];
        let mut matrix = vec![0u8; 32 * ((k >> 3) + 1)];
        let mut dec = ErasureDecoder::new(k, frag_size, &mut assembly, &mut matrix).unwrap();
        let counter = k + 1 + r;
        let st = dec.process(
            counter as u16,
            &parity[r * frag_size..(r + 1) * frag_size],
        );
        println!("decoder counter={counter} (r={r}) -> {st:?}");
        if st == DecodeStatus::Invalid {
            println!("  !! decoder rejected the fragment the encoder produced");
        }
    }
}
