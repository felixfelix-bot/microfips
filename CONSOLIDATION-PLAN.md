# Plan: microfips Git Consolidation

**Date:** 2026-07-29
**Repo:** ~/repos/microfips-upstream/
**Status:** AWAITING OPERATOR APPROVAL
**Risk:** LOW — backup exists, all operations reversible

---

## Background

Someone ran `git checkout --orphan` on local main, creating a single root commit (7e46749) disconnected from the real 565-commit history on origin/main. Local main is a strict subset of origin/main — zero unique files. The orphan commit is preserved on origin/fips-espnow-transport branch + in .git-backup-20260729-195449.

## Pre-conditions (ALREADY MET)

- [x] Backup: `.git-backup-20260729-195449/` exists and verified
- [x] All remotes fetched (origin, fork, ngit)
- [x] Forensics report: GIT-FORENSICS-REPORT.md

---

## Phase 1: Preserve Orphan Refs

Rename the orphan-pointing branches so we never lose track of them.

**Commands:**
```
git -C ~/repos/microfips-upstream branch -m main local-orphan-main
git -C ~/repos/microfips-upstream branch -m balloon-fips-lr2021 local-orphan-balloon-fips-lr2021
```

**Quality Gate 1:**
- [ ] `git branch` shows `local-orphan-main` and `local-orphan-balloon-fips-lr2021`
- [ ] No branch named `main` exists locally (temporary state)
- [ ] Both orphan branches point to 7e46749: `git log --oneline -1 local-orphan-main` confirms

**Rollback if fails:** `git branch -m local-orphan-main main` + `git branch -m local-orphan-balloon-fips-lr2021 balloon-fips-lr2021`

---

## Phase 2: Create Local Main from Real History

Point local main at origin/main (565 commits, the real project).

**Commands:**
```
git -C ~/repos/microfips-upstream checkout -b main origin/main
```

**Quality Gate 2:**
- [ ] `git log --oneline -3 main` shows b02c6b0 at HEAD
- [ ] `git log --oneline -3 origin/main` matches exactly
- [ ] `git status` shows clean working tree
- [ ] File count: `git ls-files | wc -l` returns 200 (not 156)
- [ ] The 5 differing files match origin/main versions:
  - `git diff main origin/main -- crates/microfips-core/src/noise.rs` → empty
  - `git diff main origin/main -- crates/microfips-esp-transport/Cargo.toml` → empty
  - `git diff main origin/main -- crates/microfips-esp-transport/src/lib.rs` → empty
  - `git diff main origin/main -- crates/microfips-esp-transport/src/run_tasks.rs` → empty
  - `git diff main origin/main -- crates/microfips-protocol/src/node.rs` → empty

**Rollback if fails:** `git branch -D main && git branch -m local-orphan-main main`

---

## Phase 3: Push 6 ngit-Only Branches to GitHub

These branches exist only on ngit (nostr). Push them to GitHub for backup.

**Branches:**
1. `ngit/feat/golden-vectors-and-noise-crate` → `feat/golden-vectors-and-noise-crate`
2. `ngit/feat/proto-defs-mvp` → `feat/proto-defs-mvp`
3. `ngit/feat/wifi-firmware-clean` → `feat/wifi-firmware-clean`
4. `ngit/refactor/dual-noise-ik-xx` → `refactor/dual-noise-ik-xx`
5. `ngit/fix/ble-first-heartbeat` → `fix/ble-first-heartbeat`
6. `ngit/test/echo-regression-126` → `test/echo-regression-126`

**Commands (one per branch):**
```
git -C ~/repos/microfips-upstream push origin ngit/<branch>:refs/heads/<branch>
```

**Quality Gate 3 (per branch):**
- [ ] `git ls-remote origin <branch>` returns the expected SHA
- [ ] No errors from git push
- [ ] Count: `git ls-remote --heads origin | wc -l` increased by 6

**Rollback if fails:** `git push origin --delete <branch>` (deletes the just-pushed remote branch)

---

## Phase 4: Verify All Branches on Both Remotes

Confirm nothing exists only locally or only on ngit.

**Commands:**
```
git -C ~/repos/microfips-upstream fetch --all --prune
git -C ~/repos/microfips-upstream branch -a -v > /tmp/microfips-branch-audit.txt
```

**Quality Gate 4:**
- [ ] Every ngit branch now also exists on origin (GitHub)
- [ ] Local main matches origin/main (b02c6b0)
- [ ] Orphan branches (local-orphan-*) still exist as safety net
- [ ] fips-espnow-transport matches across local/origin/ngit (870b354)
- [ ] Write branch audit to file: `cat /tmp/microfips-branch-audit.txt`

---

## Phase 5: Sync ngit/main to origin/main

ngit/main is 2 commits behind origin/main. Bring it up to date.

**Commands:**
```
git -C ~/repos/microfips-upstream checkout main
git -C ~/repos/microfips-upstream push ngit main
```

**Quality Gate 5:**
- [ ] `git log --oneline -1 ngit/main` shows b02c6b0 (matching origin)
- [ ] `git diff origin/main ngit/main` → empty

---

## Phase 6: Document Final State

Record the consolidated state.

**Output:** Update GIT-FORENSICS-REPORT.md with a "POST-CONSOLIDATION" section showing final branch map.

**Quality Gate 6:**
- [ ] Report updated with final state
- [ ] Committed to repo
- [ ] Pushed to GitHub

---

## CLEANUP (Optional — Only After Felix Confirms Everything Works)

Delete orphan branches ONLY after Felix has confirmed the repo works correctly for at least one work session.

```
git -C ~/repos/microfips-upstream branch -D local-orphan-main
git -C ~/repos/microfips-upstream branch -D local-orphan-balloon-fips-lr2021
```

Note: Even after deletion, the commits survive in:
- `.git-backup-20260729-195449/`
- `origin/fips-espnow-transport` branch ancestry

---

## Summary Table

| Phase | What | Risk | Gate |
|-------|------|------|------|
| 1 | Rename orphan branches | Zero — local only | Orphan refs exist with new names |
| 2 | Create main from origin/main | Low — reversible | main matches origin, 200 files, 5 diffs empty |
| 3 | Push 6 ngit branches to GitHub | Low — additive only | Each branch exists on GitHub |
| 4 | Verify all remotes synced | Zero — read-only | Full audit, no local-only branches |
| 5 | Sync ngit/main to latest | Low — ngit mirror | ngit/main matches origin/main |
| 6 | Document final state | Zero | Report updated + pushed |

## What NEVER Happens
- No `git push --force` to origin
- No `git reset --hard` on remote
- No history rewriting
- No deletion without explicit approval
- No work on remote without local verification first
