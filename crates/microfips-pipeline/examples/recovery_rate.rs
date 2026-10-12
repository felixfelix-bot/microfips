// Measure what the codec ACTUALLY guarantees: for k=12, redundancy=6, drop N
// fragments at random and count the recoverable fraction. This distinguishes
// "my solver is broken" from "the code cannot recover N arbitrary losses".
use microfips_pipeline::erasure::{ErasureDecoder, ErasureEncoder};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn main() {
    let (k, frag_size, redundancy) = (12usize, 48usize, 6usize);
    let total = k + redundancy;

    for losses in 1..=6usize {
        let mut ok = 0usize;
        let trials = 300usize;
        for t in 0..trials {
            // Deterministic block content.
            let mut block = vec![0u8; k * frag_size];
            for (i, b) in block.iter_mut().enumerate() {
                *b = ((i * 7 + t * 13) % 251) as u8;
            }
            let enc = ErasureEncoder::new(k, frag_size, &block).unwrap();
            let mut parity = vec![0u8; redundancy * frag_size];
            for r in 0..redundancy {
                enc.encode_redundant(r, &mut parity[r * frag_size..(r + 1) * frag_size]);
            }

            let mut rng = Rng(0x1234 + t as u64);
            let mut order: Vec<usize> = (0..total).collect();
            for i in (1..order.len()).rev() {
                let j = rng.below(i + 1);
                order.swap(i, j);
            }
            let lost: Vec<bool> = {
                let mut v = vec![false; total];
                for &idx in order.iter().take(losses) {
                    v[idx] = true;
                }
                v
            };

            let mut assembly = vec![0u8; k * frag_size + 32 * frag_size];
            let mut matrix = vec![0u8; 32 * ((k >> 3) + 1)];
            let mut dec = ErasureDecoder::new(k, frag_size, &mut assembly, &mut matrix).unwrap();

            for &idx in order.iter() {
                if lost[idx] {
                    continue;
                }
                let counter = idx + 1;
                let payload: &[u8] = if counter <= k {
                    let off = idx * frag_size;
                    &block[off..off + frag_size]
                } else {
                    &parity[(counter - k - 1) * frag_size..(counter - k) * frag_size]
                };
                dec.process(counter as u16, payload);
            }
            if dec.is_complete() {
                ok += 1;
            }
        }
        println!(
            "losses={losses}: recovered {ok}/{trials} ({:.1}%)",
            100.0 * ok as f64 / trials as f64
        );
    }
}
