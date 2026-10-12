# PROGRESS — P1.2 Pipeline TX (fragment + erasure encode + emit)

## Premise corrections (found at session start)

1. `docs/plan-espnow-fips-mesh.md` and `SPEC.md` **do not exist** in this repo.
   The task's "Reference:" is unsatisfiable. Work proceeded from the task body
   plus the existing P1.1 crate.
2. **P1.1 was already landed** on branch `feat/pipeline-rx` (commits `c1ce40f`,
   `2062e42`, `b8f0b1d`) by a prior worker, which also began P1.3 (RX).
   That worker's session is dead; its worktree `~/worktrees/pipeline-rx` holds
   uncommitted edits. My worktree is a fresh clone off that branch.
3. **The inherited branch does not compile cleanly** (pre-existing, NOT mine):
   - `crates/microfips-pipeline/Cargo.toml` declares `[[test]] pipeline_rx`
     pointing at `tests/pipeline_rx.rs`, which does not exist -> every
     `cargo test -p microfips-pipeline` fails target resolution.
   - `tests/row_weights.rs` imports `erasure::parity_row_weight`, which was
     renamed to `parity_row_bits` -> compile error.
4. **The inherited branch is RED**: `tests/erasure_vectors.rs` has 3 failing
   tests (11 pass / 3 fail) that the dead worker was mid-fixing.

## Baseline measured in MY worktree (before any P1.2 change)

    cargo test -p microfips-pipeline            -> target resolution error (item 3)
    cargo test -p microfips-pipeline --lib      -> 0 tests, ok
    cargo test -p microfips-pipeline --test erasure_vectors
        -> 11 passed; 3 FAILED:
           maximum_geometry_round_trips
           out_of_order_delivery_decodes_identically
           recovers_any_loss_pattern_the_parity_rows_can_span

## Root causes of the 3 inherited failures (diagnosed, measured)

### R1. `a` stack buffer is undersized in `ge_solve` (real memory-safety bug)
`erasure.rs:386` declares `let mut a = [0u8; MAX_FRAGS * MAX_FRAGS / 8]` = 512 B,
but `row_bytes = matrix_row_bytes(n) = (n>>3)+1` is derived from the **missing
count** `n`, not from `MAX_FRAGS`. At the max geometry (`n = 64`),
`n * row_bytes = 64 * 9 = 576 > 512` -> write past the end of a stack array.
Measured: overflow begins at `n = 64` exactly; `n <= 56` is fine.
This explains `maximum_geometry_round_trips` (k=64, lose 32 originals).

### R2. Not all of the 3 failures are bugs -- one is an over-claiming test
`out_of_order_delivery_decodes_identically` (k=8, lose originals 1,3,6) is
reproduced INCOMPLETE in an isolated harness, and `recovers_any_loss_pattern...`
fails on byte mismatch, not on completeness.
Both stem from the same pre-existing codec property that P1.1's own commit
`b8f0b1d` documents: the reference parity generator's rows have **rank m/2,
not m** (`examples/matrix_rank.rs`: m=8 -> rank 4, m=12 -> rank 6, m=32 ->
rank 16; `examples/recovery_rate.rs`: 1 loss 95.3%, 2 losses 83.3%, 3 losses
69.3%). So a block with `redundancy` parity fragments cannot generally recover
`redundancy` losses. The dead worker had already started correcting these two
tests (uncommitted edit changes `lost=[1,3,6]` -> `lost=[2]` and adds a
`columns_are_spanable` premise assertion).

### R3. Encoder/decoder parity-row alignment is CORRECT (ruled out empirically)
I suspected an off-by-one (`encode_redundant` seeds row `index+1`; the decoder
seeds row `redundancy_idx = counter - frag_nb`). A purpose-built example
(`examples/row_align.rs`, since deleted) printed both sides' rows for k=8:
they match exactly for every index. The 1-based `+1` on the encoder and the
1-based `redundancy_idx` on the decoder are the same row number. **No fix
needed** -- recorded so the next worker does not "fix" a non-bug.

## Scope decision

My task is **P1.2 (TX)**. R1 is a genuine memory-safety bug in code that P1.2
exercises end-to-end (a TX fragment round-trip through RX at max geometry hits
it), so it is IN scope and I fixed it. R2/R3 are P1.1/P1.3 artefacts on the
inherited branch; R3 needs nothing, R2 is a test-correctness issue in the
inherited suite. I do not silently rewrite the inherited worker's tests; the
status is recorded here and in REPORT.md for the manager.

## Milestones

- [x] worktree `~/worktrees/microfips-P1.2`, branch `worker-base/P1.2-pipeline-tx`
      off `feat/pipeline-rx` @ b8f0b1d
- [x] baseline measured (above)
- [x] R1/R2/R3 diagnosed with measurements
- [x] fix R1: size `a` from `MAX_FRAGS`, not `MAX_FRAGS*MAX_FRAGS/8`
- [ ] P1.2: TX module -- pad, split, erasure-encode, emit via callback
- [ ] P1.2 unit tests: fragment generation, padding, CRC, block_id, geometry
- [ ] round-trip test TX -> RX
- [ ] PROGRESS/REPORT, commit, push

## Files touched

- `crates/microfips-pipeline/src/erasure.rs` (R1 fix)
- `crates/microfips-pipeline/src/tx.rs` (P1.2, new)
- `crates/microfips-pipeline/src/lib.rs` (wire up tx module)
- `crates/microfips-pipeline/tests/pipeline_tx.rs` (P1.2 tests, new)
