#!/usr/bin/env python3
"""Tests for the opt-in smoke harness and its workflow.

The suite never starts a core or a real agent. It runs `--dry-run` with a
PATH that has no backlot binary, so the generated cast is exercised
offline; the workflow is parsed as YAML when PyYAML is available.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SMOKE = REPO / "scripts" / "smoke" / "run-smoke.sh"
WORKFLOW = REPO / ".github" / "workflows" / "nightly-smoke.yml"
NEUTRAL = "/usr/bin:/bin"
# Kept as pieces so this file never spells a real person or home path.
PRIVATE = ("follow" + "cat", "/" + "home/" + "follow" + "cat", "@" + "gmail")


def clean_env(extra=None):
    env = os.environ.copy()
    env["PATH"] = NEUTRAL
    env.pop("BACKLOT_BIN", None)
    env.pop("HC_CORE_BIN", None)
    env.pop("HC_BIN", None)
    if extra:
        env.update(extra)
    return env


def run(args, env=None, timeout=60):
    return subprocess.run(
        [str(a) for a in args],
        capture_output=True,
        text=True,
        env=env or clean_env(),
        timeout=timeout,
    )


class SmokeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="smoke-test-")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def dry_run(self, *extra):
        root = Path(self.tmp) / "root"
        return run(
            [
                SMOKE,
                "--dry-run",
                "--print-cast",
                "--root",
                root,
                *extra,
            ],
            env=clean_env({"TMPDIR": self.tmp}),
        )

    def test_shell_syntax(self):
        result = run(["bash", "-n", SMOKE])
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_dry_run_generates_a_neutral_bounded_cast(self):
        result = self.dry_run("--runner", "fake", "--model", "model-a", "--timeout", "42")
        self.assertEqual(result.returncode, 0, result.stderr)
        cast = tomllib.loads(result.stdout)
        self.assertEqual(cast["rework_cap"], 2)
        self.assertEqual(cast["stall_limit_s"], 60)
        self.assertIn("fake", cast["limits"])
        self.assertEqual(cast["producer"]["runner"], "fake")
        self.assertEqual(cast["producer"]["model"], "model-a")
        self.assertEqual(cast["producer"]["permission_profile"], "standard")
        self.assertEqual(cast["director"]["runner"], "fake")
        self.assertEqual(cast["writer"]["runner"], "fake")
        self.assertEqual(cast["candidates"][0]["name"], "actor-a")
        self.assertEqual(cast["candidates"][0]["permission_profile"], "worktree")
        self.assertIn("timeout=42", result.stderr)
        for private in PRIVATE:
            self.assertNotIn(private, result.stdout)
        self.assertFalse((Path(self.tmp) / "root").exists(), "dry run left its root")
        self.assertEqual(os.listdir(self.tmp), [], "dry run left files in TMPDIR")

    def test_runner_and_effort_are_respected(self):
        result = self.dry_run(
            "--runner", "codex", "--model", "gpt-test", "--effort", "medium"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        cast = tomllib.loads(result.stdout)
        self.assertEqual(cast["producer"]["runner"], "codex")
        self.assertEqual(cast["producer"]["model"], "gpt-test")
        self.assertEqual(cast["producer"]["effort"], "medium")
        self.assertIn("codex", cast["limits"])

        cursor = self.dry_run("--runner", "cursor", "--model", "composer-2.5")
        self.assertEqual(cursor.returncode, 0, cursor.stderr)
        cursor_cast = tomllib.loads(cursor.stdout)
        self.assertNotIn("effort", cursor_cast["producer"])

    def test_rejects_bad_inputs(self):
        cases = [
            ("--runner", "gpt"),
            ("--timeout", "nope"),
            ("--timeout", "5"),
            ("--effort", "extreme"),
            ("--runner", "cursor", "--effort", "high"),
        ]
        for extra in cases:
            result = self.dry_run("--runner", "fake", *extra)
            self.assertNotEqual(result.returncode, 0, f"accepted {extra}")

        for bad in ("/", os.environ.get("HOME", "")):
            if not bad:
                continue
            result = run([SMOKE, "--dry-run", "--root", bad], env=clean_env())
            self.assertNotEqual(result.returncode, 0, f"accepted unsafe root {bad}")
            self.assertIn("refusing", result.stderr)

    def test_workflow_is_opt_in_and_bounded(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("workflow_dispatch", text)
        self.assertIn("schedule", text)
        self.assertIn("cron:", text)
        self.assertIn("ENABLE_NIGHTLY_SMOKE", text)
        self.assertIn("timeout-minutes", text)
        self.assertIn("retention-days: 7", text)
        self.assertIn("contents: read", text)
        self.assertIn("timeout_seconds", text)

        try:
            import yaml
        except ImportError:
            yaml = None
        if yaml is not None:
            doc = yaml.safe_load(text)
            self.assertIn("workflow_dispatch", doc[True] if True in doc else doc["on"])
            self.assertIn("schedule", doc[True] if True in doc else doc["on"])
            job = doc["jobs"]["smoke"]
            gate = job["if"]
            self.assertIn("workflow_dispatch", gate)
            self.assertIn("vars.ENABLE_NIGHTLY_SMOKE == 'true'", gate)
            self.assertLessEqual(int(job["timeout-minutes"]), 30)
            perms = job.get("permissions", doc.get("permissions"))
            self.assertEqual(perms, {"contents": "read"})

    def test_no_private_data_in_the_harness(self):
        for path in (SMOKE, REPO / "scripts" / "smoke" / "README.md"):
            if not path.exists():
                continue
            text = path.read_text(encoding="utf-8")
            for private in PRIVATE:
                self.assertNotIn(private, text, f"{path} mentions a private value")


if __name__ == "__main__":
    unittest.main(verbosity=2)
