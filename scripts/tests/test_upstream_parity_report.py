"""Evidence reports must retain failures, omissions, skipped and zero-case suites."""
import importlib.util
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "parity_report", Path(__file__).resolve().parents[1] / "report-upstream-parity.py")
REPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REPORT)


def target(name, summary):
    return (f"     Running ../rd-vm/tests/demo.rs (C:/cache/debug/deps/{name}-0123456789abcdef.exe)\n"
            + summary + "\n")


class EvidenceAccounting(unittest.TestCase):
    def test_early_next_stderr_header_preserves_fifo_summary_ownership(self):
        first = target("upstream_first", "").splitlines()[0]
        second = target("shipping_strings", "").splitlines()[0]
        third = target("upstream_interrupted", "").splitlines()[0]
        log = (first + "\nrunning 5 tests\n" + second + "\n"
               "test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
               "running 2 tests\n" + third + "\n"
               "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;\n")
        suites = REPORT.parse_cargo_log(log)
        self.assertEqual([(s["target"], s["passed"], s["failed"], s["status"]) for s in suites],
                         [("upstream_first", 5, 0, "passed"),
                          ("shipping_strings", 1, 1, "failed"),
                          ("upstream_interrupted", 0, 0, "incomplete")])

    def test_zero_skipped_failed_and_interrupted_are_never_passed(self):
        log = (target("upstream_native", "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;")
               + target("upstream_partial", "test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;")
               + target("upstream_failure", "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;")
               + target("upstream_filtered", "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out;")
               + target("upstream_interrupted", "process terminated"))
        self.assertEqual([suite["status"] for suite in REPORT.parse_cargo_log(log)],
                         ["zero_cases_unverified", "partial", "failed", "partial", "incomplete"])

    def test_a_short_log_cannot_hide_missing_duplicate_or_unexpected_corpus_targets(self):
        manifest = {"test": [{"name": "upstream_one", "required-features": ["upstream-parity-tests"]},
                             {"name": "upstream_two", "required-features": ["upstream-parity-tests"]}]}
        errors = REPORT.verify_reference_routes(
            [{"target": "upstream_one"}, {"target": "upstream_one"}, {"target": "upstream_extra"}], manifest)
        self.assertEqual(errors, ["missing upstream execution: upstream_two",
                                  "unexpected upstream execution: upstream_extra",
                                  "duplicated upstream execution: upstream_one"])

    def test_compilation_or_orphan_summary_is_not_execution_evidence(self):
        for log in ("Finished `test` profile", "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"):
            with self.assertRaises(ValueError):
                REPORT.parse_cargo_log(log)

    def test_doctests_have_their_own_target_and_do_not_overwrite_integration_results(self):
        log = target("upstream_one", "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;")
        log += "   Doc-tests rd_vm\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        suites = REPORT.parse_cargo_log(log)
        self.assertEqual([(s["target"], s["passed"]) for s in suites], [("upstream_one", 3), ("doc_rd_vm", 0)])


if __name__ == "__main__":
    unittest.main()
