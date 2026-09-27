# Security policy — keys and secrets

## Rule: no host private key in `keys.json` (microfips#134)

Every device entry in `keys.json` whose `type` is `fips-host` MUST carry an
`nsec_hex` value shaped `RETRIEVE_FROM_<HOST>` (e.g. `RETRIEVE_FROM_HOST`,
`RETRIEVE_FROM_VPS`, `RETRIEVE_FROM_MAC`). A bare 64-hex value in that field is
a leaked FIPS host private key.

The mcu (`stm32`, `esp32`, `esp32s3`) and sim (`sim-a`, `sim-b`) entries are
different: they carry *published deterministic* test vectors needed by the
build (`crates/microfips-build` injects them at compile time) and by CI.

Enforced by `scripts/security/check_keys_json.py` — exit 1 on violation, exit 2
on unreadable/invalid JSON. It never prints a value, only the device name and
the value's shape. Wired in at three points:

| Point | File | Covers |
|-------|------|--------|
| commit | `.githooks/pre-commit` | the staged `keys.json` blob |
| push | `.githooks/pre-push` | every outgoing blob of `keys.json` |
| CI | `.github/workflows/ci.yml` job `secret-scan` | tip + blobs introduced by the change |

Hooks are opt-in (git only reads `core.hooksPath` when told to):

```bash
git config core.hooksPath .githooks
```

### Why not just a regex

A repository-wide `[0-9a-f]{64}` pattern is unusable: the mcu/sim vectors and
`crates/microfips-core/tests/golden_vectors.json` are 64-hex by design, so the
pattern would either block every legitimate vector edit or need an allowlist
wide enough to hide the real leak. Nor can the rule live in `.gitleaks.toml`:
gitleaks compiles rules with Go RE2, which has no lookahead, so "64-hex that is
*not* `RETRIEVE_FROM_*`" is not expressible. Hence a 60-line type-aware check.

`.gitleaks.toml` (built-in rules + the allowlists above) and
`.secret-patterns.txt` (bech32 nsecs, API-key shapes, PEM blocks, bearer
tokens, ...) remain as the generic nets they are good at. Note that the stock
gitleaks rule set does **not** flag a bare 64-hex key — that is how this class
of leak went unnoticed until the file was read by hand.

## Other rules

- Never paste a real key, nsec, or password into an issue, PR, commit message,
  review comment, report, or kanban card — not even a prefix. Refer to a leaked
  key by the *public* half (npub / node address) or say "the retired linux
  nsec".
- `keys.json` host entries are placeholders in the repository and real values in
  the operator's secret store; the lab host reads its own key from there.
- History rewriting (`git filter-repo`) is an operator decision, never
  unilateral: it rewrites every SHA and breaks every open PR and clone.
