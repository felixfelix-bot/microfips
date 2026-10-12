# PROGRESS — P1.2 Pipeline TX (fragment + erasure encode + emit)

Branch: `worker-base/P1.2-pipeline-tx` @ fork (felixfelix-bot/microfips)
Worktree: /home/c03rad0r/worktrees/microfips-P1.2

## Premise corrections (found at session start)

1. `docs/plan-espnow-fips-mesh.md` and `SPEC.md` **do not exist** in this repo.
   The task's stated references are unavailable, so the header layout and
   geometry were taken from the code that already implements them
   (`src/fragment.rs`, `src/erasure.rs`) plus the `SpiNNaker`/balloon-fresh
   erasure reference named in the crate docs. Header layout confirmed:
   `[block_id:2 BE][frag_index:1][original_count:1][crc16:2 BE]` = 6 bytes.
2. The inherited branch was **RED**: 3 failing tests + 2 test targets that
   could not build at all. Diagnosed and fixed before adding new code.

## Findings -> status

| # | Finding | Status | Files touched |
|---|---------|--------|---------------|
| F1 | `ge_solve` applied `echelon_insert`'s XOR row ops to the coefficient matrix but stored the **unreduced** rhs -> decoded garbage while reporting `complete=true` (silent corruption) | **FIXED** | `src/erasure.rs` |
| F2 | Elimination matrix `a` sized `MAX_FRAGS*MAX_FRAGS/8` = 512 B, but `row_bytes` derives from the live missing count: at n=64 it wrote 576 B (stack overflow) | **FIXED** | `src/erasure.rs` |
| F3 | `Cargo.toml` declared `[[test]] pipeline_rx` with no such source file -> the whole test build failed | **FIXED** (targets corrected to row_weights + pipeline_tx) | `Cargo.toml` |
| F4 | `tests/row_weights.rs` imported `parity_row_weight`, never exported | **FIXED** (derived from public `parity_row_bits`) | `tests/row_weights.rs` |
| F5 | `out_of_order_delivery_decodes_identically` asserted recovery for lost={1,3,6} at k=8/red=4 — **provably unsolvable** (projected rank 2 for 3 unknowns; rows {101,000,111,000}) | **FIXED** — split into a spanned-pattern success test + an unsolvable-pattern "must stay incomplete" test | `tests/erasure_vectors.rs` |
| F6 | `examples/dbg_ge.rs` needs std (`GE_TRACE`) so it broke default-feature `cargo test` | **FIXED** via `required-features = ["std"]` | `Cargo.toml`, `examples/dbg_ge.rs` |
| P1.2 | TX path itself | **DONE** | `src/tx.rs`, `src/lib.rs`, `tests/pipeline_tx.rs` |

## Deliverable status

- `Pipeline::send_frame(data, scratch, sink) -> Result<u16, TxError<E>>` — pad,
  fragment, encode, emit via `FragmentSink` callback. DONE.
- `pad_into_block` (public) — zero-pad to the block boundary. DONE.
- `TxError<E>` — no `std::io::Error`; sink error carried verbatim. DONE.
- Sink adapters: `VecSink`, `CountingSink`. DONE.
- Unit tests: **32** in `tests/pipeline_tx.rs`. DONE.

## Test evidence (observed)

```
cargo test -p microfips-pipeline               -> 15 + 32 + 1 = 48 passed, 0 failed
cargo test -p microfips-pipeline --features std -> 15 + 32 + 1 = 48 passed, 0 failed
```

Baseline before this session: `erasure_vectors` 12 passed / 3 FAILED, plus two
unbuildable test targets. After: 15/15 + 32/32 + 1/1, both feature configs.

## Commits pushed

| SHA | Subject |
|-----|---------|
| 5c30d871 | fix(pipeline): keep GE right-hand sides in lockstep with coefficient rows |
| d5960534 | feat(pipeline): add TX path -- pad, fragment, erasure-encode, emit via sink |
| a874f684 | test(pipeline): assert the codec's real loss contract; unbreak default builds |

All three verified on the remote: `git ls-remote fork worker-base/P1.2-pipeline-tx`
-> `a874f684637120046cad8128be048e7191cc8c1e`.

## Scope boundaries (honest)

- The codec's loss tolerance is **not** "any `redundancy` losses". It is exactly
  the loss patterns the parity rows span. That is an inherited P1.1 property
  (documented in `examples/matrix_rank.rs` and now asserted honestly by the
  tests). Fixing the *matrix design* to give true MDS behaviour is a larger
  change than P1.2 and is not attempted here.
- `chunking/` was not added by this task; only the TX path named in P1.2.
