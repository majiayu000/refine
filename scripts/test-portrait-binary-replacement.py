#!/usr/bin/env python3
"""Exercise only binary replacement against inert temporary executables."""

import hashlib
import os
import re
import shlex
import subprocess
import tempfile
import unittest
from pathlib import Path

SOURCE = (Path(__file__).parent / "install-local.sh").read_text(encoding="utf-8")


def function(name):
    match = re.search(rf"^{name}\(\) \{{\n.*?^\}}", SOURCE, re.MULTILINE | re.DOTALL)
    if not match:
        raise AssertionError(f"missing production function {name}")
    return match.group(0)


class PortraitBinaryReplacementTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="refine binary fixture ")
        self.root = Path(self.temp.name)
        self.cargo = self.root / "cargo bin"
        self.refine = self.root / "refine home"
        self.cargo.mkdir()
        (self.refine / "bin").mkdir(parents=True)
        self.source = self.cargo / "refine"
        self.dest = self.refine / "bin/refine-portrait"
        self.old = b"#!/bin/sh\nprintf 'old collector\\n'\n"
        self.new = b"#!/bin/sh\nprintf 'collect synthetic\\n'\n"
        self.source.write_bytes(self.new)
        self.source.chmod(0o700)
        self.dest.write_bytes(self.old)
        self.dest.chmod(0o700)

    def tearDown(self):
        self.temp.cleanup()

    def install(self, override=""):
        script = "\n".join([
            "set -euo pipefail",
            function("die"),
            function("file_sha256"),
            "log() { :; }",
            function("install_portrait_binary"),
            f"cargo_bin={shlex.quote(str(self.cargo))}",
            f"refine_dir={shlex.quote(str(self.refine))}",
            override,
            "install_portrait_binary",
            'printf "%s\\n%s\\n" "$portrait_refine_bin" "$portrait_refine_sha256"',
        ])
        return subprocess.run(
            ["bash", "-c", script], capture_output=True, text=True, timeout=10,
            env={"PATH": os.defpath, "HOME": str(self.root), "TMPDIR": str(self.root)},
        )

    def assert_preserved(self, result):
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertTrue(self.dest.is_file(), "previous executable must remain present")
        self.assertEqual(self.dest.read_bytes(), self.old)
        self.assertEqual(subprocess.check_output([str(self.dest)], text=True), "old collector\n")
        self.assertEqual(list(self.dest.parent.glob("refine-portrait.tmp.*")), [])

    def test_copy_failure_preserves_previous_executable(self):
        self.assert_preserved(self.install("cp() { return 23; }"))

    def test_partial_copy_failure_preserves_previous_executable(self):
        self.assert_preserved(self.install('cp() { printf partial > "$3"; return 23; }'))

    def test_candidate_validation_failure_preserves_previous_executable(self):
        self.source.write_bytes(b"#!/bin/sh\nprintf 'unrelated help\\n'\n")
        self.assert_preserved(self.install())

    def test_candidate_permission_failure_preserves_previous_executable(self):
        self.assert_preserved(self.install(
            'chmod() { if [[ "$2" == "$refine_dir/bin" ]]; then command chmod "$@"; else return 23; fi; }'
        ))

    def test_hash_failure_preserves_previous_executable(self):
        self.assert_preserved(self.install("file_sha256() { return 23; }"))

    def test_promotion_failure_preserves_previous_executable(self):
        self.assert_preserved(self.install("mv() { return 23; }"))

    def test_success_and_retry_promote_a_valid_independent_copy(self):
        self.assert_preserved(self.install("cp() { return 23; }"))
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.dest.read_bytes(), self.new)
        self.assertNotEqual(self.source.stat().st_ino, self.dest.stat().st_ino)
        self.assertEqual(self.dest.stat().st_mode & 0o777, 0o700)
        self.assertEqual(result.stdout.splitlines(), [str(self.dest), hashlib.sha256(self.new).hexdigest()])
        self.assertEqual(list(self.dest.parent.glob("refine-portrait.tmp.*")), [])


if __name__ == "__main__":
    unittest.main()
