"""Real AArch64 archive symbols must retain separate Linux/newlib identities."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).parents[1] / "prepare-mesa-std-link.py"
SPEC = importlib.util.spec_from_file_location("mesa_std_link", SCRIPT)
ADAPTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ADAPTER)
LLVM = Path(os.environ.get("LOCALAPPDATA", "")) / "Android/Sdk/ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64/bin"


@unittest.skipUnless((LLVM / "clang.exe").is_file(), "Pinned AArch64 LLVM tools unavailable")
class MesaAbiArchive(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="as-mesa-abi-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for stem, origin, page in (("runtime.rcgu", "rust_origin", 30), ("horizon.c", "c_origin", 8)):
            source = self.root / (stem + ".c")
            source.write_text("extern int clock_gettime(int,void*); extern long sysconf(int);\n"
                              f"long {origin}(void){{return clock_gettime(1,(void*)0)+sysconf({page});}}\n")
            self.run_tool("clang.exe", "--target=aarch64-none-elf", "-c", str(source), "-o", str(self.root / (stem + ".o")))

    def run_tool(self, tool, *args):
        return subprocess.check_output([str(LLVM / tool), *args], cwd=self.root, text=True, stderr=subprocess.STDOUT)

    def test_mixed_archive_keeps_newlib_c_and_linux_rust_symbols_separate(self):
        archive, adapted = self.root / "source.a", self.root / "adapted.a"
        self.run_tool("llvm-ar.exe", "rcs", str(archive), "runtime.rcgu.o", "horizon.c.o")
        count = ADAPTER.rewrite_public_archive(archive, adapted, LLVM / "llvm-objcopy.exe", self.root)
        self.assertEqual(count, 1)
        listing = self.run_tool("llvm-nm.exe", "--undefined-only", "--print-file-name", str(adapted))
        for symbol in ("as_mesa_clock_gettime", "as_mesa_sysconf"):
            self.assertTrue(any("runtime.rcgu.o" in line and line.endswith(symbol) for line in listing.splitlines()), listing)
        for symbol in ("clock_gettime", "sysconf"):
            self.assertTrue(any("horizon.c.o" in line and line.endswith(" " + symbol) for line in listing.splitlines()), listing)
        self.assertEqual(self.run_tool("llvm-ar.exe", "t", str(adapted)).splitlines(), ["runtime.rcgu.o", "horizon.c.o"])
        # Normal archive extraction uses the regenerated index, not --whole-archive.
        linked = self.root / "linked.o"
        self.run_tool("ld.lld.exe", "-r", "--undefined=rust_origin", "--undefined=c_origin", str(adapted), "-o", str(linked))
        symbols = self.run_tool("llvm-nm.exe", "--defined-only", str(linked))
        self.assertIn(" T rust_origin", symbols)
        self.assertIn(" T c_origin", symbols)

    def test_thin_archives_are_rejected_without_chasing_external_member_paths(self):
        archive = self.root / "thin.a"
        self.run_tool("llvm-ar.exe", "rcsT", str(archive), "runtime.rcgu.o")
        with self.assertRaisesRegex(ValueError, "full public SDK archive"):
            ADAPTER.rewrite_public_archive(archive, self.root / "out.a", LLVM / "llvm-objcopy.exe", self.root)


if __name__ == "__main__":
    unittest.main()
