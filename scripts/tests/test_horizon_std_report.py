"""Evidence failures must never become a successful std qualification."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("std_report", Path(__file__).parents[1] / "report-horizon-std.py")
REPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REPORT)


class ReportContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.run = self.root / "run"
        self.build = self.root / "build"
        self.data = self.run / "sd-result/session123"
        self.data.mkdir(parents=True)
        self.build.mkdir()
        (self.run / "androswitch-std-probe.nro").write_bytes(b"test artifact")
        digest = hashlib.sha256(b"test artifact").hexdigest()
        (self.build / "source.json").write_bytes(b"[]")
        source = hashlib.sha256(b"[]").hexdigest()
        self.write(self.build / "build.json", {"nro_sha256": digest, "source_manifest_sha256": source})
        self.receipt = {"session_id": "session123", "nro_sha256": digest, "sd_restore": "restored",
                        "phases": [{"mode": m, "marker_observed": True} for m in ("suite", "restart", "panic")]}
        self.write(self.run / "verification.json", self.receipt)
        (self.data / "results.tsv").write_text("".join(f"{key}\tpass\tevidence\n" for key in REPORT.EXPECTED))
        for mode in ("suite", "restart"):
            (self.data / f"complete-{mode}.txt").write_text(f"session123\n{mode}\n0\n")
            (self.data / f"returned-{mode}.txt").write_text("0\n")
            (self.data / f"shutdown-{mode}.txt").write_text("C atexit reached after Rust returned\n")
        (self.data / "panic.txt").write_text("intentional std probe panic: abort policy")
        (self.run / "panic.stdout.log").write_text("ServiceAm Exit: Stubbed.")

    @staticmethod
    def write(path, value):
        path.write_text(json.dumps(value))

    def test_complete_finite_contracts(self):
        result = REPORT.analyze(self.run, self.build)
        self.assertEqual(result["status"], "passed_probe_contracts")
        self.assertEqual(result["summary"], {"pass": 32, "fail": 0, "not_run": 0})
        self.assertFalse(result["product_runner_changed"])

    def test_empty_journal_is_not_a_pass(self):
        (self.data / "results.tsv").write_text("")
        result = REPORT.analyze(self.run, self.build)
        self.assertEqual(result["status"], "incomplete")
        self.assertEqual(result["summary"]["not_run"], 29)

    def test_duplicate_or_unknown_case_rejected(self):
        for text in ("io.copy\tpass\ta\nio.copy\tpass\tb", "invented\tpass\ta", "io.copy\tskipped\ta"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                REPORT.parse_cases(text)

    def test_mismatched_binary_rejected(self):
        (self.run / "androswitch-std-probe.nro").write_bytes(b"different build")
        with self.assertRaisesRegex(ValueError, "artifact hashes"):
            REPORT.analyze(self.run, self.build)

    def test_pre_panic_marker_alone_is_not_abort_evidence(self):
        (self.run / "panic.stdout.log").write_text("still running")
        result = REPORT.analyze(self.run, self.build)
        self.assertEqual(result["cases"][-1]["status"], "fail")

    def test_missing_phase_or_wrong_return_refuses_pass(self):
        self.receipt["phases"].pop()
        self.write(self.run / "verification.json", self.receipt)
        self.assertEqual(REPORT.analyze(self.run, self.build)["status"], "incomplete")
        (self.data / "returned-suite.txt").write_text("99")
        self.assertEqual(REPORT.analyze(self.run, self.build)["cases"][-3]["status"], "fail")


if __name__ == "__main__":
    unittest.main()
