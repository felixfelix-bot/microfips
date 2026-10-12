// Decisive experiment: run the balloon-fresh C erasure decoder on the exact
// case the Rust port fails (k=12, frag_size=48, redundancy=6, the deterministic
// delivery order + loss set from trial 0 of the property test).
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include "erasure.h"

struct Rng { uint64_t s; };
static uint64_t next_u64(struct Rng *r) {
    uint64_t x = r->s;
    x ^= x << 13; x ^= x >> 7; x ^= x << 17;
    r->s = x;
    return x;
}
static size_t below(struct Rng *r, size_t n) { return (size_t)(next_u64(r) % (uint64_t)n); }

int main(void) {
    const uint16_t k = 12;
    const uint8_t frag_size = 48;
    const int redundancy = 6;
    const int total = k + redundancy;

    // Same PRNG + seed as the Rust test.
    struct Rng rng = { 0xC0FFEE | 1 };
    uint8_t block[12 * 48];
    struct Rng brng = { 1000 | 1 };
    for (size_t i = 0; i < sizeof(block); i++) block[i] = (uint8_t)(next_u64(&brng) & 0xFF);

    // Encode all parity fragments.
    uint8_t parity[6][48];
    for (int r = 0; r < redundancy; r++) {
        erasure_encoder_t enc;
        erasure_encoder_init(&enc, k, frag_size, block);
        erasure_encode_redundant(&enc, (uint16_t)r, parity[r]);
    }

    // Shuffle delivery order, pick `redundancy` losses.
    int order[18];
    for (int i = 0; i < total; i++) order[i] = i;
    for (int i = total - 1; i > 0; i--) {
        int j = (int)below(&rng, (size_t)(i + 1));
        int t = order[i]; order[i] = order[j]; order[j] = t;
    }
    int lost[18] = {0};
    for (int i = 0; i < redundancy; i++) lost[order[i]] = 1;

    printf("delivery order:");
    for (int i = 0; i < total; i++) printf(" %d", order[i]);
    printf("\nlost indices:");
    for (int i = 0; i < total; i++) if (lost[i]) printf(" %d", i);
    printf("\n");

    static uint8_t assembly[ERASURE_MAX_FRAGS * 48 + ERASURE_MAX_REDUNDANCY * 48];
    static uint8_t matrix[ERASURE_MAX_REDUNDANCY * ((ERASURE_MAX_FRAGS >> 3) + 1)];
    erasure_decoder_t dec;
    erasure_decoder_init(&dec, k, frag_size, assembly, matrix);

    for (int i = 0; i < total; i++) {
        int idx = order[i];
        if (lost[idx]) continue;
        uint16_t counter = (uint16_t)(idx + 1);
        const uint8_t *payload = (counter <= k) ? &block[idx * frag_size] : parity[counter - k - 1];
        int rc = erasure_decoder_process(&dec, counter, payload);
        printf("fed counter=%2u -> rc=%d complete=%d\n", counter, rc, dec.complete);
        if (dec.complete) break;
    }

    printf("C RESULT: complete=%d\n", dec.complete);
    if (dec.complete) {
        int match = memcmp(assembly, block, sizeof(block)) == 0;
        printf("C RESULT: bytes match=%d\n", match);
    }
    return dec.complete ? 0 : 1;
}
