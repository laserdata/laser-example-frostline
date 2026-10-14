import contextlib
import importlib.machinery
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "streaming-trial"
loader = importlib.machinery.SourceFileLoader("streaming_trial_under_test", str(SCRIPT))
spec = importlib.util.spec_from_loader(loader.name, loader)
trial = importlib.util.module_from_spec(spec)
loader.exec_module(trial)


class StreamingResultTests(unittest.TestCase):
    def test_given_an_early_clean_exit_when_validated_then_should_refuse_a_full_soak_claim(self):
        result = self.run_trial(elapsed=1, fail_cleanup=False)
        self.assertFalse(result["valid"])
        self.assertIn("before its configured duration", result["error"])

    def test_given_a_cleanup_timeout_when_finalized_then_should_preserve_a_failed_result(self):
        result = self.run_trial(elapsed=61, fail_cleanup=True)
        self.assertFalse(result["valid"])
        self.assertIn("cleanup_error", result)
        self.assertTrue(result["application_report"]["complete"])

    def test_given_a_complete_soak_when_finalized_then_should_store_only_the_release_version(self):
        result = self.run_trial(elapsed=61, fail_cleanup=False, expected_exit=0,
                                version_output="frostline-demo 0.1.0+local-build\n")
        self.assertTrue(result["valid"])
        self.assertEqual(result["binary_version"], "0.1.0")
        self.assertEqual(result["binary"], Path(sys.executable).name)
        self.assertNotIn("binary_sha256", result)

    def test_given_a_version_timeout_when_started_then_should_preserve_a_failed_result(self):
        result = self.run_trial(elapsed=61, fail_cleanup=False, fail_version=True)
        self.assertFalse(result["valid"])
        self.assertIn("timed out", result["error"])
        self.assertNotIn("binary_version", result)
        self.assertNotIn("application_report", result)

    def test_given_missing_version_output_when_started_then_should_preserve_a_failed_result(self):
        result = self.run_trial(elapsed=61, fail_cleanup=False, version_output="frostline-demo\n")
        self.assertFalse(result["valid"])
        self.assertEqual(result["error"], "the application did not report a release version")
        self.assertNotIn("application_report", result)

    def run_trial(self, elapsed, fail_cleanup, expected_exit=1,
                  version_output="frostline-demo 0.1.0\n", fail_version=False):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "trial"
            arguments = [str(SCRIPT), "soak_5m", "--seconds", "60", "--binary", sys.executable,
                         "--out", str(directory)]

            socket_paths = []

            def command(args, **kwargs):
                if args[-1] == "--version":
                    if fail_version:
                        raise subprocess.TimeoutExpired(args, 10)
                    return SimpleNamespace(returncode=0, stdout=version_output)
                if args[-1] == "start":
                    state = Path(kwargs["env"]["FROSTLINE_STACK_DIR"])
                    state.mkdir(parents=True)
                    (state / "pids.json").write_text(json.dumps({"iggy": 11, "plane": 12}))
                    application = directory / "application"
                    application.mkdir()
                    (application / "report.json").write_text(json.dumps({"complete": True, "expired_windows": 0}))
                elif args[-1] == "stop":
                    socket_paths.append(Path(kwargs["env"]["FROSTLINE_PLANE_SOCKET"]).parent)
                    if fail_cleanup:
                        raise subprocess.TimeoutExpired(args, 40)
                else:
                    raise AssertionError(f"unexpected subprocess call: {args}")
                return SimpleNamespace(returncode=0)

            child = SimpleNamespace(pid=13, returncode=0, poll=lambda: 0)
            sample = {"cpu_ticks": 0, "rss_bytes": 0, "cgroup_events": {"oom_kill": 0}}
            with patch.object(sys, "argv", arguments), \
                 patch.object(trial.subprocess, "run", side_effect=command), \
                 patch.object(trial.subprocess, "Popen", return_value=child), \
                 patch.object(trial, "sample", return_value=sample), \
                 patch.object(trial.time, "monotonic", side_effect=[0, elapsed]), \
                 contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(SystemExit) as exit_status:
                    trial.main()
                self.assertEqual(exit_status.exception.code, expected_exit)
            self.assertEqual(len(socket_paths), 1)
            self.assertFalse(socket_paths[0].exists())
            return json.loads((directory / "result.json").read_text())


if __name__ == "__main__":
    unittest.main()
