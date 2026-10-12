// Measure the real bit-weight of parity rows produced by the C reference,
// to check whether the Rust port reproduces it exactly.
#include <stdio.h>
#include <stdint.h>
#include "erasure.h"

static int popcount_row(const uint8_t *row, int bytes) {
    int c = 0;
    for (int i = 0; i < bytes; i++) {
        for (int b = 0; b < 8; b++) c += (row[i] >> b) & 1;
    }
    return c;
}

int main(void) {
    int m = 12;
    for (int n = 0; n < 6; n++) {
        uint8_t row[ERASURE_MAX_FRAGS / 8 + 1] = {0};
        erasure_get_parity_matrix_row(n, m, row);
        printf("C row %d: weight=%d bits=", n, popcount_row(row, (m >> 3) + 1));
        for (int i = 0; i < m; i++) {
            if ((row[i >> 3] >> (i & 7)) & 1) printf("%d ", i);
        }
        printf("\n");
    }
    return 0;
}
