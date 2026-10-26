"""Declaration parsing and complete descriptor ordering, independent of APKs."""
import importlib.util
import re
import struct
import tempfile
import unittest
import zipfile
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "generate-android-types.py"
SPEC = importlib.util.spec_from_file_location("android_types", SCRIPT)
TYPES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TYPES)


def class_file(name, parent=None, interfaces=(), flags=1):
    constants = []

    def constant(text):
        encoded = text.encode("utf-8")
        constants.append(b"\x01" + struct.pack(">H", len(encoded)) + encoded)
        utf = len(constants)
        constants.append(b"\x07" + struct.pack(">H", utf))
        return len(constants)

    current = constant(name)
    superclass = constant(parent) if parent else 0
    interface_ids = [constant(value) for value in interfaces]
    return (b"\xca\xfe\xba\xbe\x00\x00\x00\x34" + struct.pack(">H", len(constants) + 1)
            + b"".join(constants) + struct.pack(">HHHH", flags | 0x20, current, superclass, len(interface_ids))
            + b"".join(struct.pack(">H", index) for index in interface_ids)
            + b"\x00\x00" * 3)


class AndroidDeclarations(unittest.TestCase):
    def test_classfile_declaration_retains_parents_interfaces_and_real_flags(self):
        declaration = TYPES.declaration(class_file("p/Entry", "p/Base", ["p/Callback"], flags=0x11))
        self.assertEqual(declaration["name"], "p/Entry")
        self.assertEqual(declaration["parent"], "p/Base")
        self.assertEqual(declaration["interfaces"], ["p/Callback"])
        self.assertEqual(declaration["flags"], 0x11)  # ACC_SUPER is not a Java modifier
        self.assertEqual(declaration["simple"], "Entry")

    def test_truncated_and_invalid_classfiles_fail_instead_of_emitting_metadata(self):
        good = class_file("p/Entry")
        for data in [b"bad", good[:-1], good[:10], good[:10] + b"\x00" + good[11:]]:
            with self.assertRaises(ValueError):
                TYPES.declaration(data)

    def test_generation_orders_full_descriptors_and_closes_interface_graph(self):
        with tempfile.TemporaryDirectory(prefix="androswitch-sdk-types-") as directory:
            jar = Path(directory) / "android.jar"
            with zipfile.ZipFile(jar, "w") as archive:
                for name, parent, interfaces in [
                    ("p/A", "p/Base", ["p/ChildInterface"]),
                    ("p/A$Nested", "p/A", []),
                    ("p/Base", None, []),
                    ("p/ChildInterface", None, ["p/RootInterface"]),
                    ("p/RootInterface", None, []),
                ]:
                    archive.writestr(name + ".class", class_file(name, parent, interfaces))
            source, receipt = TYPES.generate(jar)
            names = re.findall(r'Type \{ name: "(L[^\"]*;)"', source)
            self.assertEqual(len(names), 5)
            self.assertEqual(names, sorted(names))
            self.assertLess(names.index("Lp/A$Nested;"), names.index("Lp/A;"))
            entry = next(line for line in source.splitlines() if 'name: "Lp/A;"' in line)
            self.assertIn('"Lp/ChildInterface;", "Lp/RootInterface;"', entry)
            self.assertEqual(receipt["classes"], 5)


if __name__ == "__main__":
    unittest.main()
