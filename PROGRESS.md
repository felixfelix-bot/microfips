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
- [x] P1.1: erasure fidelity tests (14 tests) → tests/erasure_vectors.rs
- [ ] P1.3 test: 30% loss out-of-order recovery → tests/pipeline_rx.rs
- [ ] PROGRESS/REPORT, commit, push

## Findings

- **erasure decoder: multi-loss recovery FAILS as ported from C.**
  Repro: k=12, frag_size=48, redundancy=6, 6 losses (originals 0,4,6,9,10 +
  parity#1). Decoder never completes (`complete=false`), even though 11 of 12
  parity+original fragments arrived. Single-loss recovery works (test passes).
  This is being root-caused now; see `examples/dbg_ge.rs`.
