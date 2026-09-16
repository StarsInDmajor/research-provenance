use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use rp_scale_generate::{
    CounterStream, GenerateOptions, Profile, bounded_integer, generate, largest_remainder, typed_id,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[test]
fn sha256_counter_and_bounded_integer_match_golden_vectors() {
    let mut stream = CounterStream::new(20260830, "golden").unwrap();
    assert_eq!(
        hex(&stream.take(64)),
        concat!(
            "2e6e63ef4bf102d5b35d3a752a8ddc27ed7aa4c768d1416223b6d43015a6f667",
            "499671e41f1b7dd05a91987c4c33a088e3476ea4e591ffb8d50c8958c05fcba6"
        )
    );
    let mut stream = CounterStream::new(20260830, "golden").unwrap();
    assert_eq!(bounded_integer(&mut stream, 0, 10).unwrap(), 5);
    assert_eq!(bounded_integer(&mut stream, 0, 10).unwrap(), 9);
}

#[test]
fn typed_ulids_apportionment_and_paths_match_cross_runtime_vectors() {
    assert_eq!(
        typed_id(20260830, "rp/project/v1", "proj", 0).unwrap(),
        "proj_01KDVDNA00WGEAFZQD7NXH36WT"
    );
    assert_eq!(
        typed_id(20260830, "rp/research-run/v1", "run", 1).unwrap(),
        "run_01KDVDNA01ZC1V748637W7T3W3"
    );
    assert_eq!(
        typed_id(20260830, "rp/external-reference/v1", "ref", 2).unwrap(),
        "ref_01KDVDNA022ATYKG0ARB5YX8Q8"
    );
    let weights = BTreeMap::from([
        ("a".to_string(), 1_u64),
        ("b".to_string(), 2_u64),
        ("c".to_string(), 2_u64),
    ]);
    assert_eq!(
        largest_remainder(17, &weights).unwrap(),
        BTreeMap::from([
            ("a".to_string(), 3_usize),
            ("b".to_string(), 7_usize),
            ("c".to_string(), 7_usize),
        ])
    );
    let id = typed_id(20260830, "rp/node-revision/v1", "qst", 42).unwrap();
    assert_eq!(
        rp_scale_generate::canonical_path("Question", "question", 42, &id).unwrap(),
        format!(".research/records/questions/question-000042--{id}.yaml")
    );
}

#[test]
fn smoke_generation_is_byte_identical_manifest_excludes_itself_and_self_validates() {
    let first = temp_dir("first");
    let second = temp_dir("second");
    let first_manifest = generate(GenerateOptions {
        profile: Profile::Smoke,
        seed: 20260830,
        output: first.clone(),
    })
    .unwrap();
    let second_manifest = generate(GenerateOptions {
        profile: Profile::Smoke,
        seed: 20260830,
        output: second.clone(),
    })
    .unwrap();
    assert_eq!(
        first_manifest.aggregate_sha256,
        second_manifest.aggregate_sha256
    );
    assert_eq!(
        first_manifest.aggregate_sha256,
        "sha256:16f42cfecd487cf24375267a2271bb0cf64a752d0bf9676c13fb4ac44879ff42"
    );
    assert_eq!(
        first_manifest.object_counts,
        Profile::Smoke.canonical_counts()
    );
    assert_eq!(first_manifest.kind_counts.len(), 15);
    assert!(
        first_manifest
            .kind_counts
            .values()
            .all(|count| *count >= 10)
    );
    assert_eq!(first_manifest.relation_type_counts.len(), 24);
    assert!(
        first_manifest
            .relation_type_counts
            .values()
            .all(|count| *count > 0)
    );
    assert_eq!(
        first_manifest.access_counts,
        BTreeMap::from([
            ("exclusive".to_string(), 244),
            ("internal".to_string(), 2_926),
            ("public".to_string(), 487),
            ("restricted".to_string(), 1_219),
        ])
    );
    assert_eq!(first_manifest.sorted_path_sha256_entries.len(), 4_876 + 51);
    assert!(
        first_manifest
            .sorted_path_sha256_entries
            .iter()
            .all(|entry| entry.path != "scale-manifest.json")
    );
    assert_eq!(
        fs::read(first.join("scale-manifest.json")).unwrap(),
        fs::read(second.join("scale-manifest.json")).unwrap()
    );
    assert_eq!(
        first_manifest.sorted_path_sha256_entries[0].path,
        ".research/artifacts/artifact-000000--art_01KDVDNA0ZJN12259MQRVP61NX.yaml"
    );
    assert_eq!(
        first_manifest.sorted_path_sha256_entries[0].sha256,
        "sha256:dc6c4731721191683772d762a81a6d5b1d3a1da0d78bc25e07096cb73b1aafda"
    );
    assert_eq!(
        first_manifest.sorted_path_sha256_entries[1].sha256,
        "sha256:52ab73579800222c3d9e1fa2c1f81f6b2e09b2ed065cb86a233f83a6de209219"
    );
    assert_eq!(
        first_manifest.sorted_path_sha256_entries[2].sha256,
        "sha256:2ddbc423b18ab46a2f617433b8abe2324497ee8588ae9571a33b2d49cbb6d777"
    );
    fs::remove_dir_all(first).unwrap();
    fs::remove_dir_all(second).unwrap();
}

#[test]
fn profiles_have_frozen_counts_and_existing_output_is_rejected() {
    assert_eq!(Profile::Smoke.total_objects(), 4_876);
    assert_eq!(Profile::Workstation.total_objects(), 61_101);
    assert_eq!(Profile::Stress.total_objects(), 305_201);
    let output = temp_dir("nonempty");
    fs::write(output.join("keep"), b"keep").unwrap();
    let result = generate(GenerateOptions {
        profile: Profile::Smoke,
        seed: 20260830,
        output: output.clone(),
    });
    assert!(result.is_err());
    assert_eq!(fs::read(output.join("keep")).unwrap(), b"keep");
    fs::remove_dir_all(output).unwrap();
}

fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rp-scale-{}-{}-{}-{label}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        20260830
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
