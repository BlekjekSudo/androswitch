//! Contract tests for M12 community, governance, issue-form, and mdBook files
//! (`RustyDroid.md` Part 0.10 GH-02/GH-03/GH-04 and Part 3 §3.1).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one directory below workspace root")
        .to_path_buf()
}

fn extract_relative_md_targets(markdown: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut in_fence = false;
    for raw_line in markdown.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut rest = raw_line;
        while let Some(idx) = rest.find("](") {
            let after = &rest[idx + 2..];
            let Some(end) = after.find(')') else {
                break;
            };
            let target = after[..end].trim();
            rest = &after[end + 1..];
            if target.is_empty()
                || target.starts_with('#')
                || target.starts_with("//")
                || target.contains("://")
                || target.starts_with("mailto:")
            {
                continue;
            }
            let path_only = target.split('#').next().unwrap_or(target);
            if !path_only.is_empty() {
                targets.push(path_only.to_string());
            }
        }
    }
    targets
}

#[test]
fn m12_community_and_governance_files_exist_and_links_resolve() {
    let root = repo_root();
    let required_root_docs = [
        "CODE_OF_CONDUCT.md",
        "GOVERNANCE.md",
        "SUPPORT.md",
        "CHANGELOG.md",
        "README.en.md",
    ];

    for rel in required_root_docs {
        let path = root.join(rel);
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("expected M12 file {} to exist: {e}", path.display()));
        assert!(
            content.lines().count() >= 25,
            "expected substantive content in {rel}"
        );
        for target in extract_relative_md_targets(&content) {
            let resolved = path.parent().expect("parent").join(&target);
            assert!(
                resolved.exists(),
                "broken relative link '{target}' in {rel} -> {}",
                resolved.display()
            );
        }
    }

    let funding = fs::read_to_string(root.join(".github/FUNDING.yml"))
        .expect(".github/FUNDING.yml must exist");
    assert!(!funding.trim().is_empty());

    let perf_tpl = fs::read_to_string(root.join(".github/ISSUE_TEMPLATE/perf-regression.yml"))
        .expect(".github/ISSUE_TEMPLATE/perf-regression.yml must exist");
    assert!(perf_tpl.contains("perf-regression"));
    assert!(perf_tpl.contains("moto-e5") || perf_tpl.contains("Moto E5"));

    let labels_json =
        fs::read_to_string(root.join("docs/labels.json")).expect("docs/labels.json must exist");
    assert!(
        labels_json.contains("\"perf-regression\""),
        "perf-regression.yml label must match docs/labels.json"
    );
}

#[test]
fn mdbook_summary_and_config_are_consistent_and_resolvable() {
    let root = repo_root();
    let docs_dir = root.join("docs");

    let book_toml =
        fs::read_to_string(docs_dir.join("book.toml")).expect("docs/book.toml must exist");
    assert!(book_toml.contains("[book]"));
    assert!(book_toml.contains("src = \".\""));
    assert!(book_toml.contains("create-missing = false"));

    let summary =
        fs::read_to_string(docs_dir.join("SUMMARY.md")).expect("docs/SUMMARY.md must exist");
    let targets = extract_relative_md_targets(&summary);
    assert!(
        targets.len() >= 30,
        "expected comprehensive docs/SUMMARY.md index, got {} links",
        targets.len()
    );

    let mut seen = BTreeSet::new();
    for target in &targets {
        let resolved = docs_dir.join(target);
        assert!(
            resolved.is_file(),
            "SUMMARY.md chapter target '{target}' does not exist at {}",
            resolved.display()
        );
        let canonical = resolved
            .canonicalize()
            .unwrap_or_else(|_| resolved.to_path_buf());
        assert!(
            seen.insert(canonical),
            "duplicate chapter target '{target}' in docs/SUMMARY.md"
        );
    }

    // Core generated scoreboards and foundational guides must remain indexed.
    for core in [
        "./index.md",
        "./ROADMAP.md",
        "./compat-matrix.md",
        "./api-coverage.md",
        "./jetpack.md",
        "./gms.md",
        "./engine-registry.md",
        "./mcp-tools.md",
        "./rd-sentinel.md",
    ] {
        assert!(
            targets.iter().any(|t| t == core),
            "expected core doc '{core}' in docs/SUMMARY.md"
        );
    }
}
