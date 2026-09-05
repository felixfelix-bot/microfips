#!/usr/bin/env python3
"""Deterministic secp256k1 test-identity generator for the microfips lab bench.

Identities are secp256k1 generator * N (nsec = N in hex), matching the
device-registry.json convention (stm32=1, esp32=2, sim-a=3, sim-b=4, esp32s3=5,
esp32c3=6, esp32s3b=7, then lab assignments 8..12, see AGENTS.md).

node_addr = SHA256(x_only_pubkey)[..16]  (fips-identity NodeAddr::from_pubkey_x)

Self-checks against known vectors before printing anything.

Usage:
  lab_keygen.py 8                      # G*N identity (registry/interop/CI — PUBLIC by design)
  lab_keygen.py --seed <hex> label..   # run-scoped identity: HMAC-SHA256(seed, label) -> scalar
  lab_keygen.py --salt label..         # persistent identity: HMAC(LAB_KEY_SALT from env, label)

The G*N identities are deliberately public (reproducible interop). The
seeded identities are the bench's per-test keys (#206-class): each
scenario run derives fresh identities from a random seed recorded only in
the run's local artifacts, so published results (verdicts, console logs,
issue comments) contain single-use npubs that are dead when the run ends.
The salted identities cover long-lived non-published roles (the standard
lab daemon): not derivable from public info, but stable across restarts.
"""
import hashlib
import hmac
import json
import os
import sys

P = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F
# secp256k1 group order n (scalar field) — reduction target for derived keys.
N_ORDER = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
Gx = 0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798
Gy = 0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8


def _add(p, q):
    if p is None:
        return q
    if q is None:
        return p
    if p[0] == q[0] and (p[1] + q[1]) % P == 0:
        return None
    if p == q:
        l = (3 * p[0] * p[0]) * pow(2 * p[1], -1, P) % P
    else:
        l = (q[1] - p[1]) * pow(q[0] - p[0], -1, P) % P
    x = (l * l - p[0] - q[0]) % P
    return (x, (l * (p[0] - x) - p[1]) % P)


def _mul(k):
    r, b = None, (Gx, Gy)
    while k:
        if k & 1:
            r = _add(r, b)
        b = _add(b, b)
        k >>= 1
    return r


def identity(n: int) -> dict:
    x, y = _mul(n)
    nsec = f"{n:064x}"
    npub = f"{2 + (y & 1):02x}{x:064x}"
    addr = hashlib.sha256(x.to_bytes(32, "big")).digest()[:16].hex()
    return {"n": n, "nsec_hex": nsec, "npub_hex": npub, "node_addr": addr}


def seeded_identity(seed: bytes, label: str) -> dict:
    """Run-scoped or salted identity: scalar =
    HMAC-SHA256(seed, 'microfips-lab/<label>') reduced into [1, n-1]
    (bias < 2^-128 — negligible at secp256k1 scale). The point math is
    shared with the G*N path; only the scalar source differs."""
    d = hmac.new(seed, f"microfips-lab/{label}".encode(), hashlib.sha256).digest()
    k = (int.from_bytes(d, "big") % (N_ORDER - 1)) + 1
    x, y = _mul(k)
    return {
        "label": label,
        "nsec_hex": f"{k:064x}",
        "npub_hex": f"{2 + (y & 1):02x}{x:064x}",
        "node_addr": hashlib.sha256(x.to_bytes(32, "big")).digest()[:16].hex(),
    }


def selfcheck() -> None:
    """G*3 must match device-registry.json sim-a (cross-checked against a live daemon)."""
    id3 = identity(3)
    assert id3["npub_hex"].startswith("02f9308a019258c31049"), "G*3 x mismatch"
    assert id3["node_addr"] == "7c79f3071e28344e8153bf6c73c294eb", "G*3 addr mismatch"
    # Seeded identities: deterministic per (seed, label), distinct across
    # both axes, and high-entropy scalars (a derivation bug that collapses
    # the range must fail here, not on the bench).
    s1 = seeded_identity(bytes.fromhex("aa" * 32), "daemon")
    s2 = seeded_identity(bytes.fromhex("aa" * 32), "daemon")
    s3 = seeded_identity(bytes.fromhex("aa" * 32), "node")
    s4 = seeded_identity(bytes.fromhex("bb" * 32), "daemon")
    assert s1 == s2, "seeded identity must be deterministic"
    assert len({s1["nsec_hex"], s3["nsec_hex"], s4["nsec_hex"]}) == 3, \
        "distinct seed/label must give distinct keys"
    for ident in (s1, s3, s4):
        k = int(ident["nsec_hex"], 16)
        assert 1 <= k < N_ORDER and k > 2**240, "derived scalar out of high-entropy range"


def main() -> None:
    selfcheck()
    argv = sys.argv[1:]
    if "--seed" in argv:
        i = argv.index("--seed")
        seed_hex = argv[i + 1]
        labels = argv[i + 2:]
        if not labels or len(seed_hex) < 32 or len(seed_hex) % 2:
            sys.exit("usage: lab_keygen.py --seed <hex>=32B> <label>...")
        seed = bytes.fromhex(seed_hex)
        for label in labels:
            print(json.dumps(seeded_identity(seed, label), indent=2))
        return
    if "--salt" in argv:
        i = argv.index("--salt")
        labels = argv[i + 1:]
        salt_hex = os.environ.get("LAB_KEY_SALT", "")
        if not labels or len(salt_hex) < 64 or len(salt_hex) % 2:
            sys.exit("usage: LAB_KEY_SALT=<hex>=32B> lab_keygen.py --salt <label>...")
        salt = bytes.fromhex(salt_hex)
        for label in labels:
            print(json.dumps(seeded_identity(salt, label), indent=2))
        return
    args = [a for a in argv if a != "--env"]
    as_env = "--env" in argv
    for a in args:
        i = identity(int(a))
        if as_env:
            print(f'export DEVICE_NSEC_HEX_vps={i["nsec_hex"]}')
            print(f'export DEVICE_NPUB_HEX_vps={i["npub_hex"]}')
        else:
            print(json.dumps(i, indent=2))


if __name__ == "__main__":
    main()
