#!/usr/bin/env python3
"""Tests for the microfips host-key guard (keys.json rule, microfips#134).

Run:  python3 -m unittest discover -s scripts/security/tests -v
      (or) python3 scripts/security/tests/test_host_key_guard.py

Covers two layers:
  1. scripts/security/check_keys_json.py — the type-aware keys.json predicate.
  2. .githooks/pre-commit + .githooks/pre-push — behavioural probe: a planted
     fips-host key must be BLOCKED, the fixed tree must pass.

The tests never embed a real key: planted values are synthetic (0x11.., 0xaa..)
and the assertions check that the value itself is never echoed back.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
CHECKER = os.path.join(REPO, "scripts", "security", "check_keys_json.py")
GITHOOKS = os.path.join(REPO, ".githooks")

PLANTED = "11" * 32          # synthetic 64-hex, never a real key
PLANTED2 = "aa" * 32
RETRIEVE = "RETRIEVE_FROM_HOST"


def run_checker(*paths, extra=()):
    cmd = [sys.executable, CHECKER, *extra, *paths]
    p = subprocess.run(cmd, capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def doc(devices):
    return json.dumps({"devices": devices})


def write(tmp, name, text):
    path = os.path.join(tmp, name)
    with open(path, "w") as fh:
        fh.write(text)
    return path


# --------------------------------------------------------------------------
# 1. check_keys_json.py — predicate layer
# --------------------------------------------------------------------------
class CheckKeysJsonTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="keys-guard-")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def test_clean_document_passes(self):
        p = write(self.tmp, "keys.json", doc({
            "linux": {"type": "fips-host", "nsec_hex": RETRIEVE},
            "vps": {"type": "fips-host", "nsec_hex": "RETRIEVE_FROM_VPS"},
        }))
        rc, out = run_checker(p)
        self.assertEqual(rc, 0, out)

    def test_planted_fips_host_hex_is_blocked(self):
        p = write(self.tmp, "keys.json", doc({
            "linux": {"type": "fips-host", "nsec_hex": PLANTED},
        }))
        rc, out = run_checker(p)
        self.assertEqual(rc, 1, out)
        self.assertIn("linux", out)
        self.assertIn("LEAKED HOST PRIVATE KEY", out)
        # The guard must never echo the value it found.
        self.assertNotIn(PLANTED, out)
        self.assertNotIn(PLANTED[:12], out)

    def test_placeholder_shapes_accepted(self):
        for value in ("RETRIEVE_FROM_HOST", "RETRIEVE_FROM_VPS", "RETRIEVE_FROM_MAC",
                      "RETRIEVE_FROM_AI_LEGION_SMALL"):
            d = doc({"linux": {"type": "fips-host", "nsec_hex": value}})
            p = write(self.tmp, "keys.json", d)
            rc, out = run_checker(p)
            self.assertEqual(rc, 0, "rejected %s: %s" % (value, out))

    def test_mcu_and_sim_hex_are_not_violations(self):
        """Regression guard against the obvious false positive: the published
        deterministic test vectors (mcu/sim) legitimately hold 64-hex."""
        p = write(self.tmp, "keys.json", doc({
            "stm32": {"type": "mcu", "nsec_hex": "%064d" % 1},
            "sim-a": {"type": "sim", "nsec_hex": PLANTED},
            "linux": {"type": "fips-host", "nsec_hex": RETRIEVE},
        }))
        rc, out = run_checker(p)
        self.assertEqual(rc, 0, out)
        self.assertIn("type=sim", out)          # disclosed as a note ...
        self.assertIn("type=mcu", out)
        self.assertNotIn(PLANTED, out)          # ... without echoing the value

    def test_fips_host_without_nsec_hex_is_blocked(self):
        p = write(self.tmp, "keys.json", doc({"linux": {"type": "fips-host"}}))
        rc, out = run_checker(p)
        self.assertEqual(rc, 1, out)
        self.assertIn("no 'nsec_hex'", out)

    def test_device_without_type_is_blocked(self):
        p = write(self.tmp, "keys.json", doc({"linux": {"nsec_hex": RETRIEVE}}))
        rc, out = run_checker(p)
        self.assertEqual(rc, 1, out)
        self.assertIn("no string 'type'", out)

    def test_two_violations_are_both_reported(self):
        p = write(self.tmp, "keys.json", doc({
            "linux": {"type": "fips-host", "nsec_hex": PLANTED},
            "mac": {"type": "fips-host", "nsec_hex": PLANTED2},
        }))
        rc, out = run_checker(p)
        self.assertEqual(rc, 1, out)
        self.assertIn("linux", out)
        self.assertIn("mac", out)

    def test_invalid_json_is_a_usage_error(self):
        p = write(self.tmp, "keys.json", "{not json")
        rc, out = run_checker(p)
        self.assertEqual(rc, 2, out)

    def test_missing_file_default_is_a_usage_error(self):
        rc, out = run_checker(os.path.join(self.tmp, "nope.json"))
        self.assertEqual(rc, 2, out)

    def test_missing_file_allowed_with_flag(self):
        rc, out = run_checker(os.path.join(self.tmp, "nope.json"),
                              extra=("--allow-missing",))
        self.assertEqual(rc, 0, out)

    def test_repo_keys_json_is_clean_at_this_branch(self):
        """This branch stacks on the Tier-0 fix, so the in-tree keys.json must
        already satisfy the rule."""
        rc, out = run_checker(os.path.join(REPO, "keys.json"))
        self.assertEqual(rc, 0, out)

    def test_multiple_paths_all_checked(self):
        good = write(self.tmp, "good.json", doc({"linux": {"type": "fips-host", "nsec_hex": RETRIEVE}}))
        bad = write(self.tmp, "bad.json", doc({"linux": {"type": "fips-host", "nsec_hex": PLANTED}}))
        rc, out = run_checker(good, bad)
        self.assertEqual(rc, 1, out)
        self.assertIn("bad.json", out)


# --------------------------------------------------------------------------
# 2. hooks — behavioural probe in a throwaway repo
# --------------------------------------------------------------------------
class HookProbeTest(unittest.TestCase):
    """Install the repo's .githooks into a scratch repository and try to commit
    a planted key. This is a behavioural probe, not a text assertion: it runs
    the real hooks the same way git does."""

    def setUp(self):
        if not shutil.which("git"):
            self.skipTest("git not available")
        if not os.path.isdir(GITHOOKS):
            self.fail(".githooks/ is missing from the tree")
        self.tmp = tempfile.mkdtemp(prefix="hook-probe-")
        self.repo = os.path.join(self.tmp, "repo")
        self.remote = os.path.join(self.tmp, "remote.git")
        self._git("init", "--quiet", "--bare", self.remote)
        self._git("init", "--quiet", self.repo)
        self._git("-C", self.repo, "config", "user.email", "guard@test.invalid")
        self._git("-C", self.repo, "config", "user.name", "guard probe")
        for name in (".secret-patterns.txt", ".gitleaks.toml"):
            src = os.path.join(REPO, name)
            if os.path.exists(src):
                shutil.copy2(src, os.path.join(self.repo, name))
        # The hooks call scripts/security/check_keys_json.py, so the probe repo
        # needs it at the same relative path.
        shutil.copytree(os.path.join(REPO, "scripts", "security"),
                        os.path.join(self.repo, "scripts", "security"))
        dest = os.path.join(self.repo, ".githooks")
        os.makedirs(dest, exist_ok=True)
        for name in os.listdir(GITHOOKS):
            shutil.copy2(os.path.join(GITHOOKS, name), os.path.join(dest, name))
            os.chmod(os.path.join(dest, name), 0o755)
        self._git("-C", self.repo, "config", "core.hooksPath", ".githooks")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def _git(self, *args, **kw):
        if len(args) > 1 and args[0] == "-C":
            pass
        return subprocess.run(["git", *args], capture_output=True, text=True, **kw)

    def _commit(self, *files):
        self._git("-C", self.repo, "add", *files)
        return subprocess.run(["git", "-C", self.repo, "commit", "-m", "probe"],
                              capture_output=True, text=True)

    def test_planted_keys_json_commit_is_blocked(self):
        write(self.repo, "keys.json", doc({
            "linux": {"type": "fips-host", "nsec_hex": PLANTED},
        }))
        p = self._commit("keys.json")
        self.assertNotEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertIn("BLOCKED", (p.stdout + p.stderr))
        self.assertNotIn(PLANTED, (p.stdout + p.stderr))

    def test_clean_keys_json_commit_is_allowed(self):
        write(self.repo, "keys.json", doc({
            "linux": {"type": "fips-host", "nsec_hex": RETRIEVE},
            "stm32": {"type": "mcu", "nsec_hex": "%064d" % 2},
        }))
        p = self._commit("keys.json")
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)

    def test_planted_key_in_commit_is_blocked_at_push(self):
        """pre-push scans the outgoing history, so a --no-verify commit must
        still be caught before it leaves the machine."""
        write(self.repo, "notes.txt", "clean\n")
        self.assertEqual(self._commit("notes.txt").returncode, 0)
        write(self.repo, "keys.json", doc({
            "mac": {"type": "fips-host", "nsec_hex": PLANTED2},
        }))
        p = self._commit("keys.json")
        if p.returncode != 0:
            # pre-commit already blocked it — still assert the push path below
            p = subprocess.run(["git", "-C", self.repo, "commit", "--no-verify",
                                "-m", "probe"], capture_output=True, text=True)
        self._git("-C", self.repo, "remote", "add", "origin", self.remote)
        push = subprocess.run(["git", "-C", self.repo, "push", "origin", "HEAD:refs/heads/main"],
                              capture_output=True, text=True)
        self.assertNotEqual(push.returncode, 0, push.stdout + push.stderr)
        self.assertIn("BLOCKED", push.stdout + push.stderr)

    def test_clean_history_pushes(self):
        write(self.repo, "keys.json", doc({"linux": {"type": "fips-host", "nsec_hex": RETRIEVE}}))
        self.assertEqual(self._commit("keys.json").returncode, 0)
        self._git("-C", self.repo, "remote", "add", "origin", self.remote)
        push = subprocess.run(["git", "-C", self.repo, "push", "origin", "HEAD:refs/heads/main"],
                              capture_output=True, text=True)
        self.assertEqual(push.returncode, 0, push.stdout + push.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
