# REPORT — P1.2: Pipeline TX (fragment + erasure encode + emit)

**Status: COMPLETE and pushed.** TX path implemented, tested, and green in both
feature configurations. Three commits on `worker-base/P1.2-pipeline-tx`, all
verified present on the remote.

- Worktree: `/home/c03rad0r/worktrees/microfips-P1.2`
- Remote: `fork` = https://github.com/felixfelix-bot/microfips.git
- Branch: `worker-base/P1.2-pipeline-tx`
- Head: `a874f684` (confirmed via `git ls-remote`: `a874f684637120046cad8128be048e7191cc8c1e`)

---

## 1. What was asked

Implement the TX half of the fragmentation pipeline in the `microfips-pipeline`
crate:

- `Pipeline::send_frame(data: &[u8]) -> Result`
- Pad data to `k * block_size`
- Split into `k` fragments, 6-byte header each
  (`block_id` + `frag_index` + `original_count` + `crc16`)
- Generate `N` redundant fragments via the erasure encoder
- Emit fragments via a callback

Deliverable: TX module + unit tests for correct fragment generation.

## 2. Premise correction — the cited references do not exist

The task pointed at `docs/plan-espnow-fips-mesh.md §P1.2` and `SPEC.md §4, §6`.
**Neither file exists in this repository** (verified by search across the tree;
the only docs are `docs/NOISE_XX.md`, `docs/FSP_SESSION.md`,
`docs/microfips-protocol.md`). No `plan-espnow-fips-mesh` file exists on any
branch checked.

Rather than guess a wire format, I derived the contract from the code that
already implements it:

- `src/fragment.rs` — the 6-byte header, big-endian:
  `[block_id:2][frag_index:1][original_count:1][crc16:2]`, CRC-16/CCITT-FALSE
  over the payload only.
- `src/erasure.rs` — the PRBS23-XOR encoder geometry: exactly
  `block_size * frag_size` payload bytes, `encode_redundant(r, out)` for parity
  index `r`.

The original task text and this derivation agree on every field, so no
requirements were reinterpreted.

## 3. The inherited branch was red — fixed before adding code

The branch I inherited (`feat/pipeline-rx`) did **not** build or pass:

- `tests/erasure_vectors.rs`: **3 failures** (12 passed / 3 failed).
- `Cargo.toml` declared `[[test]] pipeline_rx` with **no such source file**,
  which fails the whole test-target build.
- `tests/row_weights.rs` imported `parity_row_weight`, which is not exported,
  so that target could not compile either.

Had I written the TX module on top of that, "tests pass" would have been
unverifiable. Diagnosing first was the cheapest path to a trustworthy result.

### F1 — silent data corruption in `ge_solve` (the serious one)

`ge_solve` builds an elimination system: a coefficient matrix `a` and a data
matrix `b`. It reduced each candidate row via `echelon_insert` — but copied the
**unreduced** right-hand side into `b`. Every stored equation therefore paired a
reduced coefficient row with a data row that had not seen the same XOR
operations, and the solve produced **wrong bytes while reporting
`complete = true`**.

This is the failure mode that matters: the RX pipeline would hand a silently
corrupted frame to the application with no error. It reproduced as
`maximum_geometry_round_trips` and `recovers_any_loss_pattern_the_parity_rows_can_span`
failing, and I confirmed the wrong-bytes-while-complete behaviour directly with
a temporary diagnostic example before fixing.

Fix: thread the same row operations through to the rhs via a callback; the
reduced rhs accumulates them and is committed to the slot only when the row is
accepted as independent.

### F2 — stack buffer overflow at max geometry

`let mut a = [0u8; MAX_FRAGS * MAX_FRAGS / 8]` = 512 bytes, but `row_bytes` is
derived from the **live** missing count, and the loop wrote `n * row_bytes`
bytes. At `n = 64`, `row_bytes = 9`, so it wrote 576 bytes into 512 — an
overflow. Fixed by sizing from `MAX_FRAGS` columns properly.

### F5 — a test asserting something mathematically impossible

`out_of_order_delivery_decodes_identically` asserted that, with `k=8`,
`redundancy=4`, losing originals `{1,3,6}` still decodes. **It cannot.**

I verified this independently with textbook Gaussian elimination (not the
crate's own solver, to avoid circular reasoning): projecting the four parity
rows onto the three missing columns gives rows `{101, 000, 111, 000}` — rank 2
for 3 unknowns. Two of the three lost originals appear in *no surviving parity
fragment at all*, so no solver can recover them.

I fixed the test to assert the two things that are actually true:

- `out_of_order_delivery_decodes_identically` — uses a loss pattern the rows
  **do** span (`{0,1,3}`) and requires byte-exact recovery.
- `an_unrecoverable_loss_pattern_never_reports_complete` — pins the `{1,3,6}`
  pattern and requires the decoder to **stay incomplete**. This is the property
  that protects the application, and it is the direct regression guard for F1.

I want to flag this explicitly rather than bury it: **the codec's loss tolerance
is not "any `redundancy` losses."** It is exactly the loss patterns the parity
rows span. That is an inherited P1.1 property (the branch's own
`examples/matrix_rank.rs` documents it). Redesigning the matrix for true MDS
behaviour is a larger change than P1.2, and I did not attempt it.

## 4. The P1.2 deliverable

New module `crates/microfips-pipeline/src/tx.rs`.

### API

```rust
Pipeline::new(config) -> Option<Pipeline>
Pipeline::send_frame(&mut self, data: &[u8], scratch: &mut [u8],
                     sink: &mut impl FragmentSink<Error = E>)
    -> Result<u16, TxError<E>>
```

Plus `pad_into_block(data, block)`, `TxError<E>`, `FragmentSink`,
`VecSink`, `CountingSink`, `tx_geometry_ok`.

### Design decisions and why

**Emission is a callback, not a returned `Vec<Vec<u8>>`.** A transmitting leaf
node hands each fragment to the radio as it is produced and may have nowhere to
keep a whole block. `send_frame` borrows the encoder directly from the caller's
scratch buffer and pushes each fragment through `FragmentSink::emit` in wire
order, releasing it immediately — at most one fragment buffer is live at a time.
The caller supplies `scratch` so the radio path needs no allocator.

**`FragmentSink` has an associated `Error` type.** A transport-specific failure
(radio status on a node, `io::Error` on a host) propagates verbatim. `TxError<E>`
deliberately does **not** wrap `std::io::Error`: this crate is `no_std` and runs
on a leaf node with no `io` layer. On sink error, emission **stops** and the
error is returned — no silent partial success.

**The block id is claimed before encoding and never rewound.** A sink can fail
part-way, leaving the first few fragments of a block on the wire. If that id
were reused, a receiver would merge those stale fragments with a later,
unrelated block carrying the same id and reconstruct a corrupted frame. So a
failed block burns its id. There is a dedicated test for this.

**Originals are emitted before parity.** A receiver that catches only the
leading fragments still has whole originals and needs no erasure decode.

**An over-long frame is rejected, never truncated.** Silently dropping the tail
of a frame is data loss reported as success.

### Padding

`pad_into_block` zero-fills to the block boundary; the real length is carried
alongside as `PaddedFrame::payload_len` so the receiver can trim it. **Documented
limitation:** a frame whose genuine payload ends in `0x00` cannot be recovered
exactly by trimming, because trailing zeros are indistinguishable from padding.
Callers needing byte-exact round-trips of arbitrary binary data must
length-delimit above this layer. The RX-side `trim_padding` carries the matching
note.

## 5. Tests

`crates/microfips-pipeline/tests/pipeline_tx.rs` — **32 tests**, all passing.

| Group | Coverage |
|-------|----------|
| Padding | zero-fill of the tail, no-op for an exact-capacity frame, no overrun of a short buffer, round trip |
| Fragment generation | exactly `k + redundancy` fragments, every fragment at `HEADER_SIZE + frag_size`, originals carry their own slice in order, originals before parity, parity indexed from `k`, **parity equals the XOR of its matrix row** (re-derived independently from the wire bytes) |
| Header fields | every fragment parses and CRC-verifies, every fragment carries `original_count`, CRC covers the payload only, one block id across all fragments |
| Block-id sequencing | advance by one, wrap at `u16::MAX`, distinct ids for consecutive frames |
| Error handling | over-long frame rejected with nothing emitted, short scratch rejected, empty frame accepted, invalid geometry yields no pipeline |
| Sink behaviour | error stops emission and propagates verbatim, **a failed block id is not reused**, counting sink agrees with vec sink, scratch reuse leaves no stale bytes |
| Round trips | no loss, a spanned loss pattern, reverse arrival order, two interleaved frames, identical payloads across block ids |
| Loss contract | recovering a spanned loss, and a rank-6 geometry recovering a 3-original loss |

## 6. Verification — observed output

**Ad-hoc verification (re-run at head `a3dc0747`, script since removed):**

A focused script at `~/.cache/hermes/tmp/hermes-verify-p12.sh` was run against
the worktree. It is *ad-hoc*, not a project-canonical gate: it re-derives the
6-byte header constant from source, builds both configurations, and asserts the
three test targets report zero failures. It does not replace the crate's suite.

```
== 1. library builds in both configurations ==
  default (no_std)                                         ok
  --features std                                           ok
== 2. header layout matches the spec (6 bytes: blk2 idx1 cnt1 crc2) ==
  HEADER_SIZE == 6                                         ok
== 3. TX test target: 32 tests, 0 failures ==
  pipeline_tx passed                                       ok
  pipeline_tx failed                                       ok
== 4. previously-failing suites are green ==
  erasure_vectors failed                                   ok   (passed=15)
  row_weights failed                                       ok   (passed=1)
== 5. default (no_std) suite green too ==
  no_std suite failed                                      ok
VERIFY: PASS
```

**Canonical suite runs (the project's own command, also observed this session):**

```
$ cargo test -p microfips-pipeline                # default, no_std
  erasure_vectors  15 passed; 0 failed
  pipeline_tx      32 passed; 0 failed
  row_weights       1 passed; 0 failed

$ cargo test -p microfips-pipeline --features std
  erasure_vectors  15 passed; 0 failed
  pipeline_tx      32 passed; 0 failed
  row_weights       1 passed; 0 failed

$ cargo build -p microfips-pipeline --all-targets --features std
  Finished (no errors)
```

**48 tests passing, 0 failing, in both configurations.** Baseline on the
inherited branch: `erasure_vectors` 12 passed / 3 FAILED plus two test targets
that could not build.

**Hygiene confirmed:** no diagnostic example was ever committed — `git log
--all` over the five scratch files (`max_geom_diag`, `ge_repro`, `rank_check`,
`tx_diag`, `row_align`) returns empty. Only the five pre-existing examples are
tracked at HEAD. Working tree clean at `18a7d29e`.

## 7. Commits (all pushed, all verified on the remote)

| SHA | Subject |
|-----|---------|
| `5c30d871` | fix(pipeline): keep GE right-hand sides in lockstep with coefficient rows |
| `d5960534` | feat(pipeline): add TX path -- pad, fragment, erasure-encode, emit via sink |
| `a874f684` | test(pipeline): assert the codec's real loss contract; unbreak default builds |

Push verification: `git ls-remote fork worker-base/P1.2-pipeline-tx` returns
`a874f684637120046cad8128be048e7191cc8c1e`. The working tree is clean.

## 8. Remaining work / handoff

Nothing in P1.2 is outstanding. Items a reviewer or the next task should know:

1. **`tests/pipeline_rx.rs` is still absent.** P1.2's TX side is exercised
   against the real `PipelineRx` in four round-trip tests inside
   `pipeline_tx.rs`, so the RX path is not untested — but a dedicated RX suite
   is still the natural next task (P1.3). I removed the dangling `[[test]]`
   declaration that named the missing file, so if that task adds the file it
   must re-add the target.
2. **Codec strength is matrix-rank bound, not `redundancy` bound.** If true MDS
   behaviour is wanted, that is a change to `get_parity_matrix_row` in P1.1
   territory, not to the TX path. The honest contract is now pinned by tests.
3. **Trailing-zero frames are not byte-exact** under padding trim (documented in
   both `tx.rs` and `trim_padding`).
4. **`examples/dbg_ge.rs` is a leftover debug harness** from the inherited
   branch. I made it buildable (gated behind `std`) rather than delete it; it has
   no test value now that the property test is green.
5. **No PR opened.** The task said ship, and I pushed the branch; opening a PR
   against the fork was not requested and the base branch (`feat/pipeline-rx`) is
   not yet on the fork's main line.
