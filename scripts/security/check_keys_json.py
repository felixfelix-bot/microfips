#!/usr/bin/env python3
"""Enforce the microfips host-key rule on keys.json (microfips#134).

THE RULE
--------
Every device entry whose ``type`` is ``fips-host`` MUST carry an ``nsec_hex``
value shaped ``RETRIEVE_FROM_<HOST>``.  A bare 64-hex value in that field is a
leaked FIPS host private key: it was published anonymously via
``raw.githubusercontent.com/c03rad0r/microfips/main/keys.json`` (issue #134
Tier 0, retired by ``bbc890646a5e...`` -> ``RETRIEVE_FROM_HOST``).

WHY A TYPE-AWARE CHECK AND NOT A GREP PATTERN
---------------------------------------------
A repository-wide ``[0-9a-f]{64}`` pattern is unusable here: ``keys.json``
legitimately carries published *deterministic* test vectors for the ``mcu``
(``stm32``/``esp32``/``esp32s3``) and ``sim`` devices, and the golden-vector
JSON files are full of 64-hex by design.  Adding such a pattern to
``.secret-patterns.txt`` would either block every legitimate vector edit or
have to be muted with an allowlist that swallows the real leak too.  So the
host-key rule is enforced *semantically*: parse the document, look at
``type``, allowlist exactly the ``RETRIEVE_FROM_*`` shape.  The blunt pattern
set in ``.secret-patterns.txt`` keeps doing what it is good at (bech32 nsecs,
API-key shapes, PEM blocks, bearer tokens, ...).

USAGE
-----
    scripts/security/check_keys_json.py [--allow-missing] [--quiet] PATH...
    scripts/security/check_keys_json.py -          # read one document on stdin

PATH may be a file, a directory (``keys.json`` inside it is checked), or ``-``
for stdin (use it for a *staged* blob: ``git show :keys.json | ... -``).

EXIT CODES
----------
    0  clean
    1  rule violation
    2  usage / unreadable file / invalid JSON

SECRET HANDLING
---------------
This script never prints an ``nsec_hex`` value, not even a prefix: violations
report the device name, its type and the value's *length*.  Keep it that way —
its output is pasted into CI logs, review comments and kanban cards.
"""
import argparse
import json
import os
import re
import sys

RETRIEVE_RE = re.compile(r"^RETRIEVE_FROM_[A-Z0-9_]+$")
HEX64_RE = re.compile(r"^[0-9a-fA-F]{64}$")
HOST_TYPE = "fips-host"


def _shape(value):
    """Describe a value without disclosing it."""
    if not isinstance(value, str):
        return type(value).__name__
    if HEX64_RE.match(value):
        return "64-hex (len=%d)" % len(value)
    if RETRIEVE_RE.match(value):
        return "RETRIEVE placeholder"
    return "string len=%d" % len(value)


def check_document(doc, label):
    """Return (problems, notes). problems -> exit 1, notes -> informational."""
    problems, notes = [], []
    if not isinstance(doc, dict):
        return ["%s: top level is not a JSON object" % label], notes
    devices = doc.get("devices")
    if not isinstance(devices, dict):
        return ["%s: missing or non-object 'devices'" % label], notes
    if not devices:
        problems.append("%s: 'devices' is empty" % label)
    for name in sorted(devices):
        entry = devices[name]
        where = "%s: devices.%s" % (label, name)
        if not isinstance(entry, dict):
            problems.append("%s is not an object" % where)
            continue
        dtype = entry.get("type")
        if not isinstance(dtype, str) or not dtype:
            problems.append(
                "%s has no string 'type' — the fips-host rule cannot be applied; "
                "add a type (fips-host|mcu|sim) or the entry escapes the guard" % where
            )
            continue
        value = entry.get("nsec_hex")
        if dtype == HOST_TYPE:
            if not isinstance(value, str) or not value:
                problems.append("%s (fips-host) has no 'nsec_hex' string" % where)
                continue
            if RETRIEVE_RE.match(value):
                continue
            shape = _shape(value)
            if HEX64_RE.match(value):
                problems.append(
                    "%s (fips-host) carries a bare 64-hex nsec_hex (%s) — LEAKED HOST "
                    "PRIVATE KEY; replace with RETRIEVE_FROM_<HOST> "
                    "(microfips#134)" % (where, shape)
                )
            else:
                problems.append(
                    "%s (fips-host) nsec_hex is neither RETRIEVE_FROM_<HOST> nor "
                    "64-hex (%s) — refuse to guess" % (where, shape)
                )
        else:
            if isinstance(value, str) and HEX64_RE.match(value):
                notes.append(
                    "%s (type=%s) carries a 64-hex nsec_hex (%s) — allowed only for "
                    "published deterministic test vectors; outside the fips-host rule"
                    % (where, dtype, _shape(value))
                )
    return problems, notes


def _iter_documents(paths):
    for path in paths:
        if path == "-":
            yield "<stdin>", sys.stdin.read()
            continue
        if os.path.isdir(path):
            path = os.path.join(path, "keys.json")
        if not os.path.exists(path):
            yield path, None
            continue
        with open(path, "r", encoding="utf-8") as fh:
            yield path, fh.read()


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Enforce the microfips host-key rule on keys.json (microfips#134)")
    ap.add_argument("paths", nargs="*", default=["keys.json"],
                    help="files, directories or '-' for stdin (default: keys.json)")
    ap.add_argument("--allow-missing", action="store_true",
                    help="a path that does not exist is not an error")
    ap.add_argument("--quiet", action="store_true", help="suppress notes")
    args = ap.parse_args(argv)

    paths = args.paths or ["keys.json"]
    problems, notes, errors = [], [], []
    checked = 0
    for label, text in _iter_documents(paths):
        if text is None:
            if args.allow_missing:
                continue
            errors.append("%s: no such file" % label)
            continue
        try:
            doc = json.loads(text)
        except json.JSONDecodeError as exc:
            errors.append("%s: invalid JSON: %s" % (label, exc))
            continue
        checked += 1
        p, n = check_document(doc, label)
        problems.extend(p)
        notes.extend(n)

    if not args.quiet:
        for note in notes:
            print("note: %s" % note)
    for problem in problems:
        print("error: %s" % problem)
    for error in errors:
        print("error: %s" % error)

    if errors:
        print("FAIL (usage/parse) — %d document(s) checked" % checked)
        return 2
    if problems:
        print("FAIL — %d host-key violation(s) in %d document(s)" % (len(problems), checked))
        return 1
    if not args.quiet:
        print("OK — keys.json host-key rule satisfied (%d document(s) checked)" % checked)
    return 0


if __name__ == "__main__":
    sys.exit(main())
