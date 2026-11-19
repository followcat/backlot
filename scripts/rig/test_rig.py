#!/usr/bin/env python3
"""Tests for the isolated two-machine rig (scripts/rig/).

The suite never starts a core or a GUI: it checks syntax, the plan
output, argument refusals, and that `down` kills exactly the recorded
PID and removes the root with no leftovers.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
RIG = REPO / "scripts" / "rig" / "rig.sh"
XCLICK = REPO / "scripts" / "rig" / "xclick.py"


def clean_env(extra=None):
    env = os.environ.copy()
    env.pop("RIG_DISPLAY", None)
    env.pop("RIG_ROOT", None)
    env.pop("RIG_MACHINES", None)
    if extra:
        env.update(extra)
    return env


def run(args, env=None, timeout=30):
    return subprocess.run(
        [str(a) for a in args],
        capture_output=True,
        text=True,
        env=env or clean_env(),
        timeout=timeout,
    )


class RigTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="rig-test-")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def test_shell_syntax(self):
        result = run(["bash", "-n", RIG])
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_plan_validates_and_prints_isolated_world(self):
        root = Path(self.tmp) / "rig-plan"
        result = run(
            [
                RIG,
                "plan",
                "--root",
                root,
                "--display",
                ":90",
                "--machines",
                "3",
                "--seed-latest",
                "v9.9.9",
            ]
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        out = result.stdout
        for needle in (
            "root=" + str(root),
            "machines=3",
            "display=:90",
            "host-b",
            "host-c",
            "acme-shop",
            "beta-tools",
            "seed=" + str(root) + "/state-a/backlot/update-check.json tag=v9.9.9",
        ):
            self.assertIn(needle, out)
        for var in (
            "HC_SCOPE",
            "HC_TERMINAL_ID",
            "HC_TERMINAL_SOCKET",
            "HERDR_ENV",
            "WAYLAND_DISPLAY",
            "BACKLOT_STUDIO",
            "BACKLOT_ROLE",
            "BACKLOT_TASK",
            "BACKLOT_STATE_DIR",
        ):
            self.assertIn(var, out)
        self.assertFalse(root.exists(), "plan must not create the root")

    def test_plan_defaults_to_two_machines(self):
        root = Path(self.tmp) / "rig-two"
        result = run([RIG, "up", "--dry-run", "--root", root, "--display", ":91"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("machines=2", result.stdout)
        self.assertIn("host-b", result.stdout)
        self.assertNotIn("host-c", result.stdout)

    def test_refuses_unsafe_roots(self):
        for bad in ("/", os.environ.get("HOME", "")):
            if not bad:
                continue
            result = run([RIG, "plan", "--root", bad])
            self.assertNotEqual(result.returncode, 0, f"accepted unsafe root {bad}")
            self.assertIn("refusing", result.stderr)

    def test_rejects_bad_seed_latest(self):
        result = run([RIG, "plan", "--root", Path(self.tmp) / "r", "--seed-latest", "banana"])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("seed-latest", result.stderr)

    def test_down_kills_only_the_recorded_pid(self):
        root = Path(self.tmp) / "rig-down"
        root.mkdir()
        child = subprocess.Popen(["sleep", "300"])
        bystander = subprocess.Popen(["sleep", "300"])
        try:
            (root / "pids").write_text(f"sleep {child.pid}\n", encoding="utf-8")
            result = run(
                [RIG, "down", "--root", root],
                env=clean_env({"TMPDIR": self.tmp}),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            child.wait(timeout=5)
            self.assertIsNotNone(child.returncode)
            self.assertIsNone(bystander.poll(), "down killed an unrecorded process")
            self.assertFalse(root.exists(), "down left the root behind")
            self.assertEqual(
                os.listdir(self.tmp), [], "down left files in TMPDIR"
            )
        finally:
            for proc in (child, bystander):
                if proc.poll() is None:
                    proc.kill()
                proc.wait(timeout=5)

    def test_xclick_dry_run_and_display_requirement(self):
        clicked = run([sys.executable, XCLICK, "--display", ":90", "--dry-run", "click", "10", "20", "--button", "3"])
        self.assertEqual(clicked.returncode, 0, clicked.stderr)
        self.assertIn("click :90 10 20 button=3", clicked.stdout)

        dragged = run([sys.executable, XCLICK, "--display", ":90", "--dry-run", "drag", "1", "2", "30", "40"])
        self.assertEqual(dragged.returncode, 0, dragged.stderr)
        self.assertIn("drag :90 1 2 30 40", dragged.stdout)

        missing = run([sys.executable, XCLICK, "--dry-run", "click", "1", "2"])
        self.assertNotEqual(missing.returncode, 0)
        self.assertIn("RIG_DISPLAY", missing.stderr)

        from_env = run(
            [sys.executable, XCLICK, "--dry-run", "click", "5", "6"],
            env=clean_env({"RIG_DISPLAY": ":92"}),
        )
        self.assertEqual(from_env.returncode, 0, from_env.stderr)
        self.assertIn("click :92 5 6", from_env.stdout)

    def test_rig_has_no_global_kill_commands(self):
        banned = ["pk" + "ill", "kill" + "all"]
        for path in (REPO / "scripts" / "rig").rglob("*"):
            if not path.is_file() or "__pycache__" in path.parts:
                continue
            if path.suffix not in (".sh", ".py", ".md"):
                continue
            text = path.read_text(encoding="utf-8", errors="ignore").lower()
            for word in banned:
                self.assertNotIn(word, text, f"{path} contains {word}")


if __name__ == "__main__":
    unittest.main(verbosity=2)
