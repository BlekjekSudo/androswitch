"""Omission/drift checks must fail even when the remaining files look valid."""
import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "check-upstream-sync.py"
SPEC = importlib.util.spec_from_file_location("upstream_sync", SCRIPT)
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class UpstreamAudit(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="androswitch-upstream-audit-")
        self.root = Path(self.temp.name).resolve()
        self.assertTrue(self.root.name.startswith("androswitch-upstream-audit-"))
        self.addCleanup(self.temp.cleanup)
        (self.root / "one.rs").write_bytes(b"one\n")
        self.entry = {"path": "one.rs", "blob": AUDIT.blob_oid(b"one\n")}

    def test_missing_file_is_not_an_unsupported_success(self):
        _, errors = AUDIT.verify_entries(self.root, [{**self.entry, "path": "absent.rs"}])
        self.assertIn("UPSTREAM_SOURCE_MISSING", errors[0])

    def test_source_drift_requires_a_reviewed_hash_and_reason(self):
        (self.root / "one.rs").write_bytes(b"changed\n")
        _, errors = AUDIT.verify_entries(self.root, [self.entry])
        self.assertIn("UNREVIEWED_ADAPTATION", errors[0])
        import hashlib
        approved = {**self.entry, "reason": "actual adapter", "sha256": hashlib.sha256(b"changed\n").hexdigest()}
        self.assertEqual(AUDIT.verify_entries(self.root, [approved])[1], [])
        (self.root / "one.rs").write_bytes(b"changed again\n")
        self.assertIn("ADAPTATION_DRIFT", AUDIT.verify_entries(self.root, [approved])[1][0])

    def test_relocation_preserves_blob_and_newline_variants(self):
        (self.root / "copy.rs").write_bytes(b"one\r\n")
        self.assertEqual(AUDIT.verify_entries(self.root, [{**self.entry, "local_path": "copy.rs"}])[1], [])

    def test_duplicate_and_repository_escape_are_rejected(self):
        self.assertIn("DUPLICATE_UPSTREAM_PATH", AUDIT.verify_entries(self.root, [self.entry, self.entry])[1][0])
        with self.assertRaises(ValueError):
            AUDIT.verify_entries(self.root, [{**self.entry, "local_path": "../outside"}])

    def test_host_cfg_does_not_become_device_parity(self):
        vm = self.root / "crates/rd-vm/src"
        vm.mkdir(parents=True)
        (vm / "lib.rs").write_text('#[cfg(feature = "std")]\npub mod arrays;\npub mod heap;\n', encoding="utf-8")
        routes = AUDIT.vm_routes(self.root, {"crates/rd-vm/src/arrays.rs", "crates/rd-vm/src/heap.rs"})
        self.assertEqual(routes[0]["device_route"], "host-only-original")
        self.assertEqual(routes[1]["device_route"], "shared-source-with-adapters")
        self.assertTrue(all(r["behavior_parity"] == "unverified" for r in routes))

    def test_experimental_alias_does_not_become_a_shipping_or_behavior_pass(self):
        vm = self.root / "crates/rd-vm/src"
        vm.mkdir(parents=True)
        (vm / "lib.rs").write_text(
            '#[cfg(feature = "std")]\npub mod intrinsics;\n'
            '#[cfg(feature = "upstream-runtime")]\n#[path = "intrinsics.rs"]\npub(crate) mod shared_intrinsics;\n'
            '#[cfg(not(feature = "std"))]\n#[path = "portable/intrinsics.rs"]\npub mod intrinsics;\n',
            encoding="utf-8")
        routes = AUDIT.vm_routes(self.root, {"crates/rd-vm/src/intrinsics.rs"})
        self.assertEqual(len(routes), 1)
        self.assertEqual(routes[0]["device_route"], "host-only-original")
        self.assertTrue(routes[0]["experimental_original_declared"])
        self.assertEqual(routes[0]["behavior_parity"], "unverified")

    def test_shortened_lock_does_not_hide_omission_from_actual_git_tree(self):
        # A real local Git tree, not an expected-array mirror of the checker.
        AUDIT.git(self.root, "init", "--quiet")
        AUDIT.git(self.root, "add", "one.rs")
        AUDIT.git(self.root, "-c", "user.name=Audit test", "-c", "user.email=audit@example.invalid", "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "fixture")
        revision = AUDIT.git(self.root, "rev-parse", "HEAD")
        vm = self.root / "crates/rd-vm/src"
        vm.mkdir(parents=True)
        (vm / "lib.rs").write_text("", encoding="utf-8")
        lock = {"revision": revision, "tree": AUDIT.git(self.root, "show", "-s", "--format=%T", revision), "files": []}
        result = AUDIT.audit(self.root, lock)
        self.assertIn("UPSTREAM_MANIFEST_INCOMPLETE_OR_CHANGED", result["errors"])
        self.assertFalse(result["source_sync_pass"])
        self.assertFalse(result["device_behavior_parity_pass"])

    def test_cargo_boundary_rejects_source_copies_and_desktop_feature_leakage(self):
        for name in ["as-vm", "as-vm-testsupport", "as-framework", "as-switch-smoke"]:
            folder = self.root / "crates" / name
            folder.mkdir(parents=True)
            source = "../rd-vm/src/lib.rs" if name == "as-vm" else "../rd-testsupport/src/lib.rs"
            (folder / "Cargo.toml").write_text(
                '[lib]\npath = "' + source + '"\n[dependencies]\nrd-vm = { package = "as-vm" }\n', encoding="utf-8")
        self.assertEqual(AUDIT.verify_vm_boundary(self.root), [])
        core = self.root / "crates/as-vm/Cargo.toml"
        core.write_text(core.read_text(encoding="utf-8").replace("../rd-vm/src/lib.rs", "src/copied.rs") + "[features]\nstd = []\n", encoding="utf-8")
        failures = AUDIT.verify_vm_boundary(self.root)
        self.assertTrue(any("DUPLICATED_UPSTREAM_SOURCE" in error for error in failures))
        self.assertIn("DESKTOP_FEATURE_EXPOSED_ON_DEVICE_VM", failures)
        consumer = self.root / "crates/as-framework/Cargo.toml"
        consumer.write_text('[dependencies]\nrd-vm = { path = "../rd-vm" }\n', encoding="utf-8")
        self.assertIn("DESKTOP_VM_DEPENDENCY_LEAK: as-framework", AUDIT.verify_vm_boundary(self.root))

    def test_extracted_helpers_cannot_evade_the_upstream_file_inventory(self):
        import hashlib
        folder = self.root / "crates/rd-vm/src"
        folder.mkdir(parents=True)
        helper = folder / "shared.rs"
        helper.write_bytes(b"derived\n")
        self.assertIn("FORK_CORE_MANIFEST_INCOMPLETE_OR_CHANGED", AUDIT.verify_fork_core(self.root, set(), []))
        entry = {"path": "crates/rd-vm/src/shared.rs", "sha256": hashlib.sha256(b"derived\n").hexdigest()}
        self.assertEqual(AUDIT.verify_fork_core(self.root, set(), [entry]), [])
        helper.write_bytes(b"unreviewed change\n")
        self.assertIn("FORK_CORE_ADAPTATION_DRIFT", AUDIT.verify_fork_core(self.root, set(), [entry])[0])

    def test_entire_upstream_corpus_must_have_unique_isolated_routes(self):
        folder = self.root / "crates/as-vm"
        folder.mkdir(parents=True)
        manifest = folder / "Cargo.toml"
        original = {"crates/rd-vm/tests/first.rs", "crates/rd-vm/tests/second.rs",
                    "crates/rd-vm/tests/common/mod.rs"}
        first = '\n[[test]]\nname="upstream_first"\npath="../rd-vm/tests/first.rs"\nrequired-features=["upstream-parity-tests"]\n'
        manifest.write_text(first, encoding="utf-8")
        routes, errors = AUDIT.upstream_test_routes(self.root, original)
        self.assertEqual(len(routes), 2)
        self.assertIn("second.rs", errors[0])
        second = first.replace("first", "second")
        manifest.write_text(first + second, encoding="utf-8")
        routes, errors = AUDIT.upstream_test_routes(self.root, original)
        self.assertEqual(errors, [])
        self.assertTrue(all(r["execution"] == "unverified" for r in routes))
        manifest.write_text(first + second + first, encoding="utf-8")
        self.assertTrue(AUDIT.upstream_test_routes(self.root, original)[1])
        manifest.write_text((first + second).replace('required-features=["upstream-parity-tests"]', ''), encoding="utf-8")
        self.assertTrue(all("NOT_ISOLATED" in e for e in AUDIT.upstream_test_routes(self.root, original)[1]))


if __name__ == "__main__":
    unittest.main()
