// Standalone debug harness: reproduce trial 0 from the failing property test
// and dump decoder state at each step.
use microfips_pipeline::erasure::{
    DecodeStatus, ErasureDecoder, ErasureEncoder, MAX_REDUNDANCY,
};

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self { Rng(seed | 1) }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        self.0 = x; x
    }
    fn below(&mut self, n: usize) -> usize { (self.next_u64() % n as u64) as usize }
}

fn make_block(k: usize, frag_size: usize, seed: u64) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    (0..k * frag_size).map(|_| (rng.next_u64() & 0xFF) as u8).collect()
}

fn main() {
    microfips_pipeline::GE_TRACE.store(true, std::sync::atomic::Ordering::Relaxed);
    let (k, frag_size, redundancy) = (12usize, 48usize, 6usize);
    let total = k + redundancy;
    let mut rng = Rng::new(0xC0FFEE);

    let block = make_block(k, frag_size, 1000);
    let enc = ErasureEncoder::new(k, frag_size, &block).unwrap();
    let parity: Vec<Vec<u8>> = (0..redundancy)
        .map(|r| { let mut o = vec![0u8; frag_size]; assert!(enc.encode_redundant(r, &mut o)); o })
        .collect();

    let mut order: Vec<usize> = (0..total).collect();
    for i in (1..order.len()).rev() {
        let j = rng.below(i + 1);
        order.swap(i, j);
    }
    println!("delivery order: {:?}", order);

    let lost: Vec<usize> = { (0..total).filter(|_| false).collect() };
    let _ = lost;
    let mut lost_v = vec![false; total];
    for &idx in order.iter().take(redundancy) { lost_v[idx] = true; }
    let lost_idx: Vec<usize> = (0..total).filter(|i| lost_v[*i]).collect();
    println!("lost indices: {:?} ({} of {})", lost_idx, lost_idx.len(), total);
    println!("originals lost: {:?}", lost_idx.iter().filter(|i| **i < k).collect::<Vec<_>>());
    println!("parity lost idx: {:?}", lost_idx.iter().filter(|i| **i >= k).map(|i| i - k).collect::<Vec<_>>());

    let mut assembly = vec![0u8; k * frag_size + MAX_REDUNDANCY * frag_size];
    let mut matrix = vec![0u8; MAX_REDUNDANCY * ((k >> 3) + 1)];
    let mut dec = ErasureDecoder::new(k, frag_size, &mut assembly, &mut matrix).unwrap();

    for &idx in order.iter() {
        if lost_v[idx] { continue; }
        let counter = idx + 1;
        let payload: &[u8] = if counter <= k {
            let off = idx * frag_size; &block[off..off + frag_size]
        } else {
            &parity[counter - k - 1]
        };
        let st = dec.process(counter as u16, payload);
        println!("fed counter={counter:2} (idx={idx:2}) -> {st:?} complete={}", dec.is_complete());
        if dec.is_complete() { break; }
    }

    println!("FINAL complete={}", dec.is_complete());
    if let Some(a) = dec.assembled() {
        println!("match={}", a == &block[..]);
        // Report which fragments are still missing.
        for i in 0..k {
            let off = i * frag_size;
            let got = dec.original(i);
            match got {
                Some(g) => println!("  orig {i:2}: {}", if g == &block[off..off+frag_size] { "OK" } else { "WRONG" }),
                None => println!("  orig {i:2}: MISSING"),
            }
        }
    } else {
        println!("no assembled buffer");
    }
}
