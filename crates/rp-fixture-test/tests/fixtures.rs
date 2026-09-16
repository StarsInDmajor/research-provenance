use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use rp_fixture_test::{HarnessError, run_fixture};
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

#[test]
fn all_thirty_eight_overlays_match_exact_declared_expectations() {
    let root = fixtures_root();
    let mut descriptors = Vec::new();
    for suite in fs::read_dir(&root).unwrap() {
        let mutations = suite.unwrap().path().join("mutations");
        if !mutations.is_dir() {
            continue;
        }
        for descriptor in fs::read_dir(mutations).unwrap() {
            let path = descriptor.unwrap().path();
            if path.extension().and_then(|value| value.to_str()) == Some("yaml") {
                descriptors.push(path);
            }
        }
    }
    descriptors.sort();
    assert_eq!(descriptors.len(), 38);
    for descriptor in descriptors {
        let baseline = descriptor.parent().unwrap().parent().unwrap().join("valid");
        let outcome = run_fixture(&baseline, &descriptor)
            .unwrap_or_else(|error| panic!("{}: {error}", descriptor.display()));
        assert!(
            outcome.expectation_matched,
            "{}: {outcome:?}",
            descriptor.display()
        );
        assert_eq!(outcome.observed_exit_code, 1, "{}", descriptor.display());
        assert_eq!(outcome.materialization_mode, 0o700);
    }
}

#[test]
fn materialization_is_isolated_and_does_not_modify_the_baseline() {
    let root = fixtures_root();
    let baseline = root.join("overview-v1/valid");
    let descriptor = root.join("overview-v1/mutations/overlay-dangling-reference.yaml");
    let before = tree_digest(&baseline);
    let first = run_fixture(&baseline, &descriptor).unwrap();
    let second = run_fixture(&baseline, &descriptor).unwrap();
    assert_eq!(before, tree_digest(&baseline));
    assert_eq!(first.observed_error_codes, second.observed_error_codes);
}

#[test]
fn descriptor_schema_and_single_mutation_cardinality_are_enforced() {
    let root = fixtures_root();
    let baseline = root.join("overview-v1/valid");
    let original =
        fs::read_to_string(root.join("overview-v1/mutations/overlay-dangling-reference.yaml"))
            .unwrap();

    let invalid = temp_file("invalid-descriptor.yaml");
    fs::write(&invalid, format!("{original}\nunregistered: true\n")).unwrap();
    assert!(matches!(
        run_fixture(&baseline, &invalid),
        Err(HarnessError::DescriptorInvalid(_))
    ));

    let cardinality = temp_file("cardinality-descriptor.yaml");
    let descriptor = original
        .replace("mode: structured_ast", "mode: raw_text")
        .replace("operation: set_value", "operation: text_replace")
        .replace(
            "  path: /to_revision\n  value: hyp_01J00000000000000000000099",
            "  search: revision\n  replace: relation",
        );
    fs::write(&cardinality, descriptor).unwrap();
    assert!(matches!(
        run_fixture(&baseline, &cardinality),
        Err(HarnessError::MutationCardinality)
    ));
    let _ = fs::remove_file(invalid);
    let _ = fs::remove_file(cardinality);
}

fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rp-fixture-test-{}-{}-{name}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

fn tree_digest(root: &Path) -> String {
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
    }
    format!("{:x}", hasher.finalize())
}

fn collect_files(root: &Path, directory: &Path, output: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, output);
        } else {
            output.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                fs::read(path).unwrap(),
            ));
        }
    }
}
