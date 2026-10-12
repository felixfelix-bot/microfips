# PROGRESS — P1.3 Pipeline RX (t_c5dbc0b0)

## PREMISE CORRECTIONS (found at session start)

1. `docs/plan-espnow-fips-mesh.md` and `SPEC.md` **do not exist** in this repo.
   The task's "Reference" section is unsatisfiable. Work proceeded from the task
   body itself plus the C reference implementation.
2. **P1.1 (dependency) was never landed.** `microfips-pipeline` does not exist in
   any branch; the P1.1 worktree
   (`/home/c03rad0r/.hermes/profiles/manager/scripts/.worktrees/t_1893bdd7`) was
   deleted without a push. Its board card is `archived`/done, so the loss was
   silent. **P1.1's deliverable had to be redone here** or P1.3 had no codec to
   build on. Source of truth used: balloon-fresh C at
   `~/repos/balloon-fresh/tracker/firmware/components/erasure/erasure.c` (325 lines)
   + `include/erasure.h` (37 lines).

## Milestones

- [x] worktree `~/worktrees/pipeline-rx`, branch `feat/pipeline-rx` off `main` @ f540546
- [x] P1.1: `microfips-pipeline` crate + PRBS23-XOR erasure port (no_std, alloc)
      → files: crates/microfips-pipeline/{Cargo.toml,src/erasure.rs,src/fragment.rs,src/lib.rs}
- [x] P1.1: erasure fidelity tests → tests/erasure_vectors.rs
- [x] commit `c1ce40f` — P1.1 crate + port + fidelity tests
- [x] F1/F2 root-caused with measurements (see Findings)
- [ ] strengthen RX solver past the C's F2 defect
- [ ] P1.3: Pipeline::recv_frame() RX path → src/lib.rs
- [ ] P1.3 test: 30% loss out-of-order recovery → tests/pipeline_rx.rs
- [ ] PROGRESS/REPORT, commit, push

## Findings

### F1. The reference codec's parity matrix has rank m/2, not m (major)
`get_parity_matrix_row(n, m)` loops `while nb_coeff < m/2` — it performs `m/2`
LFSR *draws* and ORs each draw's column into the row. Draws repeat, so rows are
lighter than nominal and the `redundancy = m/2` rows span only `m/2` dimensions.

Measured (`examples/matrix_rank.rs`):
    m= 8 rows=4  weights=[4,3,4,3]        rank=4  (need 8)
    m=12 rows=6  weights=[3,5,5,4,5,5]    rank=6  (need 12)
    m=16 rows=8  weights=[8,7,7,6,6,7,7,8] rank=8 (need 16)
    m=24 rows=12 rank=12                          (need 24)
    m=32 rows=16 rank=16                          (need 32)

End-to-end recovery, k=12 / redundancy=6, 300 random trials per loss count
(`examples/recovery_rate.rs`):
    losses=1: 95.3%   losses=2: 83.3%   losses=3: 69.3%
    losses=4: 51.3%   losses=5: 31.0%   losses=6: 12.0%
A correct erasure code with 6 parity fragments recovers **100%** of 1..6 losses.
This is a property of the reference codec, NOT of the Rust port.

### F2. The C decoder runs its solver only inside the parity branch
`erasure_decoder_process` calls `ge_solve` only when appending a *redundancy*
fragment, and only when `m2b_rows >= frag_nb_lost` at that instant. Late
originals never re-trigger it, so otherwise-solvable blocks stay incomplete.
Reproduced in the C itself: `tools/c_ref_check.c` fails the exact case the Rust
port fails.

### F3. Rust port status
Faithful to C for the encoder, PRBS23 stepper and row generator. The RX solver
was strengthened past the C: rows are stored as permanent full-fidelity
equations, re-projected against the live missing set on every attempt, both
solver passes loop until no progress, and rows are selected by rank via
incremental echelon insertion. This removes F2; F1 still caps recovery, so the
P1.3 acceptance test must be scaled to the codec's real strength.
