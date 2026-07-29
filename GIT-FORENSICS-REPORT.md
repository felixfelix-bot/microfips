# Git Forensics Report — microfips-upstream

**Date:** 2026-07-29  
**Repository:** ~/repos/microfips-upstream/  
**Backup:** `.git-backup-20260729-195449` (verified intact)  
**Operation:** READ-ONLY analysis — no pushes, resets, or deletions performed

---

## 1. Repository State Summary

### Remotes (3)

| Remote | URL | Notes |
|--------|-----|-------|
| `origin` | `https://github.com/felixfelix-bot/microfips.git` | Primary GitHub remote |
| `fork` | `https://github.com/felixfelix-bot/microfips.git` | Same URL as origin (duplicate) |
| `ngit` | `nostr://npub1xh6.../relay.ngit.dev/microfips` | Nostr git mirror |

### Local Branches (3)

| Branch | Commit | Description |
|--------|--------|-------------|
| `main` (HEAD) | `7e46749` | **Orphan root commit** — "cleanup: Reset repository to clean state" |
| `balloon-fips-lr2021` | `7e46749` | **Same orphan commit** as main |
| `fips-espnow-transport` | `870b354` | Real branch, also exists on remotes |

### Remote Branches

**origin/fork (identical — same GitHub repo):** 19 branches each
- `main` → `b02c6b0` (565 commits of real work)
- `copilot/add-wifi-connection-for-esp32`, `copilot/fix-issue-61`
- `dependabot/cargo/{bt-hci-0.8.1,clap-4.6.1,defmt-1.1.0,heapless-0.9.3,tokio-1.52.3}`
- `feat/{erasure-port,esp32-wifi-modular,espnow-binary,espnow-transport,fips-v0-compat,lr2021-transport,mac-mapping,noise-xx-handshake}`
- `fips-espnow-transport`, `microfips-esp32-component`, `pr-132`, `ws/microfips`

**ngit (Nostr mirror):** 18 branches — superset of origin branches plus 4 ngit-only:
- `feat/golden-vectors-and-noise-crate`, `feat/proto-defs-mvp`, `feat/wifi-firmware-clean`
- `refactor/dual-noise-ik-xx`, `fix/ble-first-heartbeat`, `test/echo-regression-126`
- `ngit/main` → `2fd6915` (2 commits behind origin/main)

### Tags
**None** (0 tags)

---

## 2. The Divergence Explained

### What Happened

The repository has **two completely disconnected git histories** with no common ancestor:

#### Lineage A — The Real Project History (origin/main)
- **Root commit:** `ecf5c99` "Initial scaffold: Embassy USB CDC ACM echo with LED blink (M0+M1)"
- **HEAD:** `b02c6b0` "feat(esp-now): fix EspNowTransport trait impl + ESP32-C3 binary wiring"
- **Commit count:** 565 commits
- **Content:** Full microfips project — ESP-NOW transport, Noise IK/XX handshake, L2CAP BLE, MMP protocol, STM32 firmware, ESP32-C3 firmware, labgrid tests, ADRs, tools, scripts
- **Where it lives:** `origin/main`, `fork/main`, `ngit/main` (2 behind)

#### Lineage B — The Orphan Reset (local main)
- **Root commit:** `7e46749` "cleanup: Reset repository to clean state" — **NO PARENT, orphan/root commit**
- **Author:** c03rad0r, timestamp ~July 2025
- **Commit count:** 1 commit (on main; 2 commits on fips-espnow-transport which builds on top)
- **Content:** 156 files — a static snapshot of code with no git history
- **Where it lives:** Local `main`, local `balloon-fips-lr2021`, remote `origin/fips-espnow-transport` (with 1 additional commit `870b354`)

### How It Happened (Inferred)

Someone ran `git checkout --orphan` (or `git commit --orphan`) to create a clean root commit from a working tree snapshot. This discarded all 565 commits of history, replacing it with a single "reset" commit. Local `main` was then pointed at this orphan commit. The remote `origin/main` was never overwritten — it still has the full 565-commit history.

### Divergence Numbers

| Metric | Count |
|--------|-------|
| Commits local main has that origin/main doesn't | **1** (the orphan 7e46749) |
| Commits origin/main has that local main doesn't | **565** |
| Merge base between local main and origin/main | **NONE** (disconnected) |

---

## 3. Content of the Local "Reset" Commit (7e46749)

### What It Contains

The orphan commit `7e46749` is a **root commit** (no parent) containing 156 files / 33,129 lines. It is NOT an empty/destructive commit in itself — it has real code. However, it **lost all git history** (blame, log, bisect capability for the original 565 commits).

### Critical Finding: Local Main is a STRICT SUBSET of origin/main

| Metric | Count |
|--------|-------|
| Files unique to local main (not in origin/main) | **0** |
| Files unique to origin/main (not in local main) | **44** |
| Files in both but with different content | **5** |
| Files identical in both | **107** |

**Every single file in local main also exists in origin/main.** The orphan commit contains nothing unique.

#### Files only in origin/main (missing from local main) — 44 files:
- ESP32-C3 firmware: `crates/microfips-esp32c3/` (5 files)
- ESP transport: `crates/microfips-esp-transport/src/esp_now_transport.rs`
- Config: `.cargo/config.toml`, `.gitignore`, `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml`, `Makefile`, `README.md`
- Docs: `docs/architecture.md`, `docs/ble-capture-decrypt.md`, `docs/device-identity.md`, `docs/fips-microfips-parity.md`, `docs/http-demo.md`, `docs/milestones.md`
- Scripts: `scripts/` (7 shell scripts)
- Tools: `tools/` (13 Python/Lua/shell tools + reference.pcap)
- Other: `AGENTS.md`, `keys.json`, `.env.example`, `.fips-upstream.json`, `.github/dependabot.yml`

#### Files with different content (exist in both, but origin/main has newer versions):
1. `crates/microfips-core/src/noise.rs`
2. `crates/microfips-esp-transport/Cargo.toml`
3. `crates/microfips-esp-transport/src/lib.rs`
4. `crates/microfips-esp-transport/src/run_tasks.rs`
5. `crates/microfips-protocol/src/node.rs`

---

## 4. Local-Only Branches (At Risk)

| Branch | Commit | On Remote? | Risk |
|--------|--------|------------|------|
| `main` | `7e46749` | **NO** — origin/main points to `b02c6b0` | The orphan commit IS preserved on `origin/fips-espnow-transport`, but local main pointing here is wrong |
| `balloon-fips-lr2021` | `7e46749` | **NO** — does not exist on any remote as a branch | Commit is preserved via fips-espnow-transport, but the branch name is local-only |
| `fips-espnow-transport` | `870b354` | **YES** — exists on origin, fork, ngit | ✅ Safe |

**Note:** The orphan commit `7e46749` is NOT lost — it exists as an ancestor of `origin/fips-espnow-transport`. But local `main` pointing at it instead of the real history is the problem.

---

## 5. Remote-Only Branches (Not Checked Out Locally)

All 19 origin branches minus the 1 local `fips-espnow-transport` = **18 remote-only branches**. Key ones:
- `origin/main` (the real main — `b02c6b0`)
- `origin/feat/lr2021-transport`, `origin/feat/noise-xx-handshake`, `origin/feat/mac-mapping`
- `origin/feat/espnow-transport`, `origin/feat/erasure-port`, `origin/feat/esp32-wifi-modular`
- `origin/feat/fips-v0-compat`, `origin/ws/microfips`, `origin/pr-132`
- All dependabot and copilot branches

**ngit-only branches (not on origin/fork):**
- `ngit/feat/golden-vectors-and-noise-crate` → `0a5142d`
- `ngit/feat/proto-defs-mvp` → `623785f`
- `ngit/feat/wifi-firmware-clean` → `3df50c7`
- `ngit/refactor/dual-noise-ik-xx` → `a913eaa`
- `ngit/fix/ble-first-heartbeat` → `231a00a`
- `ngit/test/echo-regression-126` → `9e74e39`
- `ngit/feat/fips-v0-compat` → `46b309b` (origin version is at `22360c9` — different commit)

---

## 6. RECOMMENDATION

### Which version should be main?

**origin/main (`b02c6b0`) should absolutely be the canonical main.**

Reasons:
1. ✅ It has the full 565-commit history with proper root (`ecf5c99`)
2. ✅ It is a strict **superset** of local main — contains all 156 files PLUS 44 additional files
3. ✅ The 5 differing files are newer/more advanced versions in origin/main
4. ✅ Local main (`7e46749`) contains **zero unique files** — nothing would be lost
5. ✅ The orphan commit's "reset" message explicitly says it removed content
6. ❌ Local main has no history — no `git blame`, no `git bisect`, no provenance

### The orphan commit is safe to abandon because:
- It's preserved in `.git-backup-20260729-195449`
- It's preserved as an ancestor of `origin/fips-espnow-transport`
- It contains nothing not already in origin/main

---

## 7. CONSOLIDATION PLAN (Step-by-Step, No Data Loss)

> **⚠️ DO NOT execute yet — this is a plan for review and approval.**

### Phase 1: Safety (Already Done)
- [x] Backup created: `.git-backup-20260729-195449`
- [x] All remotes fetched
- [x] Full state documented in this report

### Phase 2: Preserve Local-Only Branch Names
```bash
# Rename local main to preserve the orphan state as a named ref
git branch -m main local-orphan-main

# Rename balloon-fips-lr2021 for clarity (it's the same orphan commit)
git branch -m balloon-fips-lr2021 local-orphan-balloon-fips-lr2021
```

### Phase 3: Reset Local Main to Real History
```bash
# Point local main at origin/main (the real 565-commit history)
# This does NOT affect the remote — it only changes the local pointer
git checkout main  # (after rename in Phase 2, use: git checkout -b main origin/main)
git branch -f main origin/main
git checkout main
```

### Phase 4: Verify
```bash
# Confirm main now matches origin/main
git log --oneline -5 main
git log --oneline -5 origin/main
# Should be identical: b02c6b0 at HEAD

# Confirm working tree is clean
git status

# Confirm orphan branches still exist as safety net
git branch | grep local-orphan
```

### Phase 5: Push ngit-Only Branches to GitHub (Optional)
The ngit-only branches (`feat/golden-vectors-and-noise-crate`, `feat/proto-defs-mvp`, etc.) should be pushed to origin/fork so they're backed up on GitHub:
```bash
# For each ngit-only branch
git push origin ngit/feat/golden-vectors-and-noise-crate:refs/heads/feat/golden-vectors-and-noise-crate
git push origin ngit/feat/proto-defs-mvp:refs/heads/feat/proto-defs-mvp
# ... etc for each ngit-only branch
```

### Phase 6: Clean Up (Only After Verification + Approval)
```bash
# Once confident everything is correct, the orphan branches can be deleted
# But ONLY after explicit approval — they're harmless to keep
git branch -d local-orphan-main  # -d (safe delete, only if merged)
```

### What NOT to Do
- ❌ **NO `git push --force`** — origin/main is correct as-is
- ❌ **NO `git reset --hard`** on the remote
- ❌ **NO branch deletion** until everything is verified
- ❌ **NO rebase** that would rewrite origin/main's history

---

## Appendix A: Full Branch Inventory

### Local Branches
```
* main                    7e46749 cleanup: Reset repository to clean state (ORPHAN)
  balloon-fips-lr2021     7e46749 cleanup: Reset repository to clean state (ORPHAN, same as main)
  fips-espnow-transport   870b354 docs: add AGENTS.md with anti-coordination guardrails
```

### Remote Branches (origin = fork, identical)
```
origin/main                         b02c6b0 feat(esp-now): fix EspNowTransport trait impl
origin/copilot/add-wifi-...         1f35138 refactor: minimal WiFi test firmware
origin/copilot/fix-issue-61         ec289f4 fix(protocol): harden epoch serialization
origin/dependabot/cargo/bt-hci-0.8.1  a01e146
origin/dependabot/cargo/clap-4.6.1     2dff0ba
origin/dependabot/cargo/defmt-1.1.0    0c64bfe
origin/dependabot/cargo/heapless-0.9.3 3a4d867
origin/dependabot/cargo/tokio-1.52.3   0853913
origin/feat/erasure-port            5f92080 feat: ESP-NOW transport for ESP32-C3 (Phase 0)
origin/feat/esp32-wifi-modular      4406fa7 fix(wifi): retain WifiController
origin/feat/espnow-binary           2fd6915 docs: add upstream compat notes
origin/feat/espnow-transport        5b6cc50 Implement ESP-NOW transport trait
origin/feat/fips-v0-compat          22360c9 Review fixes: drop unsafe Peripherals::steal
origin/feat/lr2021-transport        8355ac5 docs: update cross-track analysis
origin/feat/mac-mapping             962657c feat(routing): implement ESP-NOW MAC mapping
origin/feat/noise-xx-handshake      22cd015 docs: add comprehensive FIPS VPS interop test plan
origin/fips-espnow-transport        870b354 docs: add AGENTS.md
origin/microfips-esp32-component    fba7446 feat(esp32): add microfips-esp32-component
origin/pr-132                       22cd015 docs: add comprehensive FIPS VPS interop test plan
origin/ws/microfips                 55b6cd7 fix(esp32c3): resolve ESP-NOW binary compilation errors
```

### ngit-Only Branches (not on GitHub)
```
ngit/feat/golden-vectors-and-noise-crate  0a5142d refactor: extract Noise protocol into standalone crate
ngit/feat/proto-defs-mvp                  623785f Consume fips-proto-defs crate
ngit/feat/wifi-firmware-clean             3df50c7 feat(esp32c3): add ESP32-C3 crate + ESP-NOW transport
ngit/refactor/dual-noise-ik-xx            a913eaa style: cargo fmt
ngit/fix/ble-first-heartbeat              231a00a fix(ble): send first heartbeat 1s after steady
ngit/test/echo-regression-126             9e74e39 test(core): add echo response regression tests
ngit/feat/fips-v0-compat                  46b309b docs: add comprehensive ESP32 status report (different from origin's 22360c9)
```

---

## Appendix B: Disconnected History Diagram

```
LINEAGE A (Real History — origin/main):
  ecf5c99 "Initial scaffold: Embassy USB CDC ACM echo..."
    └── 565 commits ──→ b02c6b0 "feat(esp-now): fix EspNowTransport trait impl"
                         ↑
                    origin/main, fork/main
                    ngit/main (at 2fd6915, 2 behind)


LINEAGE B (Orphan Reset — local main):
  7e46749 "cleanup: Reset repository to clean state" (ROOT, no parent)
    └── 870b354 "docs: add AGENTS.md"
                  ↑
       local main ←──── points HERE (WRONG)
       local balloon-fips-lr2021 ←── points HERE (same orphan)
       origin/fips-espnow-transport ←── branch exists on remote

NO COMMON ANCESTOR between Lineage A and Lineage B.
```

---

*Report generated 2026-07-29. All operations were read-only. Backup at `.git-backup-20260729-195430`.*

---

## 8. POST-CONSOLIDATION STATE (2026-07-29)

All 6 phases of the consolidation plan have been executed. Summary below.

### Phase Results

| Phase | Operation | Gate | Result |
|-------|-----------|------|--------|
| 1 | Renamed orphan branches to safety-net names | Both exist in orphan lineage, no `main` | ✅ PASS |
| 2 | Created local `main` from `origin/main` (b02c6b0) | 200 files, clean tree, 0 diffs vs origin | ✅ PASS |
| 3 | Pushed 6 ngit-only branches to GitHub | All 6 SHAs verified on origin | ✅ PASS |
| 4 | Verified all branches synced | No local-only branches except orphan net | ✅ PASS |
| 5 | Sync ngit/main to origin/main | ngit/main still at 2fd6915 | ❌ FAIL (permissions) |
| 6 | Document final state | This section | ✅ PASS |

### Phase 5 Blocker

Pushing to ngit failed with: `your nostr account npub1xtzgnzzu... isn't listed as a maintainer of the repo`. The ngit repo owner (`npub1xh6...3xqw3`) must add our account as a maintainer before ngit/main can be synced. This is **non-destructive** — ngit/main is simply 2 commits behind origin/main.

### Final Branch Map

#### Local Branches (4)
| Branch | Commit | Description |
|--------|--------|-------------|
| `main` (HEAD) | `b02c6b0` | Real 565-commit history, matches origin/main |
| `fips-espnow-transport` | `870b354` | Real branch, matches origin |
| `local-orphan-main` | `0c8fb3e` | **Safety net** — orphan lineage (7e46749 → 0c8fb3e) |
| `local-orphan-balloon-fips-lr2021` | `7e46749` | **Safety net** — orphan root commit |

#### Origin (GitHub) — 26 branches
Previously 20 branches. Added 6 from ngit backup:
- `feat/golden-vectors-and-noise-crate` (0a5142d)
- `feat/proto-defs-mvp` (623785f)
- `feat/wifi-firmware-clean` (3df50c7)
- `refactor/dual-noise-ik-xx` (a913eaa)
- `fix/ble-first-heartbeat` (231a00a)
- `test/echo-regression-126` (9e74e39)

#### ngit (Nostr mirror) — 17 branches
ngit/main remains at `2fd6915` (2 behind origin/main `b02c6b0`). All other branches match origin.

### What Changed
1. Local `main` now points to real history (`b02c6b0`, 565 commits, 200 files)
2. Orphan branches preserved as `local-orphan-*` safety net
3. 6 ngit-only branches backed up to GitHub (origin)
4. All origin branches are canonical and correct

### Remaining Actions
- **Phase 5 fix:** Add nostr account as ngit maintainer, then `git push ngit main`
- **Cleanup (after Felix confirms):** `git branch -D local-orphan-main local-orphan-balloon-fips-lr2021`

---

*Post-consolidation report updated 2026-07-29. All phases executed per CONSOLIDATION-PLAN.md.*
