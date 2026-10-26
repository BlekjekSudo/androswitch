#!/usr/bin/env python3
"""Generate declaration metadata, never executable Android implementations.

SDK input is explicit and hashed. The runtime embeds only names, inheritance,
interfaces, and modifiers; an SDK declaration does not implement its methods.
"""
import argparse
import hashlib
import json
import struct
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class Reader:
    def __init__(self, data):
        self.data, self.at = data, 0

    def take(self, size):
        end = self.at + size
        if size < 0 or end > len(self.data):
            raise ValueError("truncated JVM declaration")
        result = self.data[self.at:end]
        self.at = end
        return result

    def u1(self):
        return self.take(1)[0]

    def u2(self):
        return struct.unpack(">H", self.take(2))[0]

    def u4(self):
        return struct.unpack(">I", self.take(4))[0]


def declaration(data):
    r = Reader(data)
    if r.take(4) != b"\xca\xfe\xba\xbe":
        raise ValueError("not a JVM class")
    r.take(4)  # minor/major version
    count = r.u2()
    pool = [None] * count
    i = 1
    while i < count:
        tag = r.u1()
        if tag == 1:
            pool[i] = r.take(r.u2())
        elif tag == 7:
            pool[i] = r.u2()
        elif tag in (3, 4, 9, 10, 11, 12, 17, 18):
            r.take(4)
        elif tag in (5, 6):
            r.take(8)
            i += 1
        elif tag in (8, 16, 19, 20):
            r.take(2)
        elif tag == 15:
            r.take(3)
        else:
            raise ValueError(f"unsupported JVM constant tag {tag}")
        i += 1

    def name(index):
        try:
            item = pool[index]
            if not isinstance(item, int):
                raise ValueError("expected CONSTANT_Class")
            text = pool[item]
            if not isinstance(text, bytes):
                raise ValueError("expected CONSTANT_Utf8")
            return text.decode("utf-8")
        except (IndexError, UnicodeError) as error:
            raise ValueError("invalid class-name constant") from error

    flags, this, parent = r.u2(), r.u2(), r.u2()
    result = {"name": name(this), "parent": name(parent) if parent else None,
              "interfaces": [name(r.u2()) for _ in range(r.u2())],
              "flags": flags & ~0x20, "simple": None, "outer": None}
    for _ in range(2):  # fields, methods; skip their implementation/attributes
        for _ in range(r.u2()):
            r.take(6)
            for _ in range(r.u2()):
                r.take(2)
                r.take(r.u4())
    for _ in range(r.u2()):
        attribute = pool[r.u2()]
        body = Reader(r.take(r.u4()))
        if attribute != b"InnerClasses":
            continue
        for _ in range(body.u2()):
            inner, outer, simple, inner_flags = body.u2(), body.u2(), body.u2(), body.u2()
            if inner == this:
                result["flags"] = inner_flags
                result["simple"] = pool[simple].decode("utf-8") if simple else ""
                result["outer"] = name(outer) if outer else None
    if result["simple"] is None:
        result["simple"] = result["name"].rsplit("/", 1)[-1]
    return result


def descriptor(name):
    return f"L{name};"


def generate(jar):
    classes = {}
    with zipfile.ZipFile(jar) as archive:
        for path in sorted(archive.namelist()):
            if path.endswith(".class"):
                item = declaration(archive.read(path))
                if item["name"] in classes:
                    raise ValueError("duplicate SDK class")
                classes[item["name"]] = item

    def parents(name):
        found, seen = [], {name}
        while name in classes and classes[name]["parent"]:
            name = classes[name]["parent"]
            if name in seen:
                raise ValueError("cyclic SDK superclass")
            seen.add(name)
            found.append(name)
        return found

    def interfaces(name):
        seen, result, pending = set(), [], [name]
        while pending:
            current = pending.pop()
            if current in seen:
                continue
            seen.add(current)
            item = classes.get(current)
            if item is None:
                continue
            for interface in item["interfaces"]:
                if interface not in result:
                    result.append(interface)
            pending.extend(reversed(item["interfaces"]))
            if item["parent"]:
                pending.append(item["parent"])
        return sorted(result)

    def strings(items):
        return "&[" + ", ".join(json.dumps(descriptor(item)) for item in items) + "]"

    sha = hashlib.sha256(jar.read_bytes()).hexdigest()
    lines = ["//! Generated Android SDK declaration metadata; no method bodies or API promises.",
             f"//! Input android.jar SHA-256: {sha}",
             "//! Regenerate with scripts/generate-android-types.py; see data/android-sdk-types.json.",
             "pub(crate) struct Type {",
             "    pub(crate) name: &'static str,",
             "    pub(crate) parents: &'static [&'static str],",
             "    pub(crate) interfaces: &'static [&'static str],",
             "    pub(crate) flags: u32,",
             "    pub(crate) simple: &'static str,",
             "}",
             "pub(crate) fn lookup(name: &str) -> Option<&'static Type> {",
             "    TYPES",
             "        .binary_search_by(|t| t.name.cmp(name))",
             "        .ok()",
             "        .map(|i| &TYPES[i])",
             "}",
             "#[rustfmt::skip]", "static TYPES: &[Type] = &["]
    for name, item in sorted(classes.items(), key=lambda item: descriptor(item[0])):
        lines.append("    Type { name: " + json.dumps(descriptor(name)) + ", parents: " + strings(parents(name))
                     + ", interfaces: " + strings(interfaces(name)) + f", flags: {item['flags']}, simple: "
                     + json.dumps(item["simple"]) + " },")
    lines.extend([
        "];", "", "#[cfg(test)]", "mod tests {", "    #[test]",
        "    fn every_generated_declaration_is_reachable_by_exact_descriptor() {",
        "        for pair in super::TYPES.windows(2) {",
        "            assert!(pair[0].name < pair[1].name);", "        }",
        "        for declaration in super::TYPES {",
        "            assert!(core::ptr::eq(",
        "                super::lookup(declaration.name).expect(\"generated descriptor\"),",
        "                declaration", "            ));", "        }", "    }", "}",
    ])
    metadata = {"schema": 1, "sdk_api": 35, "source": "Android SDK platform android-35/android.jar",
                "source_sha256": sha, "classes": len(classes),
                "purpose": "declaration metadata only; runtime API availability is independently implemented and verified",
                "source_code_included": False}
    return "\n".join(lines) + "\n", metadata


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--android-jar", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    source, metadata = generate(args.android_jar)
    output = ROOT / "crates/rd-vm/src/sdk_types.rs"
    receipt = ROOT / "data/android-sdk-types.json"
    encoded = json.dumps(metadata, indent=2) + "\n"
    if args.check:
        if output.read_text(encoding="utf-8") != source or receipt.read_text(encoding="utf-8") != encoded:
            raise SystemExit("Android declaration metadata differs from pinned SDK input")
    else:
        output.write_text(source, encoding="utf-8", newline="\n")
        receipt.write_text(encoded, encoding="utf-8", newline="\n")
    print(f"{metadata['classes']} declarations; input {metadata['source_sha256']}")
