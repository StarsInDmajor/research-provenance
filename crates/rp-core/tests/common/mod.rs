#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use rp_core::{ProjectPath, YamlLimits, parse_restricted_yaml};
use serde_json::Value;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .join("valid")
}

pub struct TempProject {
    root: PathBuf,
}

impl TempProject {
    pub fn empty() -> Self {
        let root = unique_temp_path();
        fs::create_dir(&root).expect("create empty temp root");
        Self { root }
    }

    pub fn copy_fixture(name: &str) -> Self {
        let root = unique_temp_path();
        fs::create_dir_all(&root).expect("create temporary project");
        copy_tree(&fixture_root(name), &root);
        Self { root }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn research(&self, relative: &str) -> PathBuf {
        self.root.join(".research").join(relative)
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn parse_yaml_file(path: &Path) -> Value {
    let bytes = fs::read(path).expect("read YAML file");
    parse_restricted_yaml(
        &bytes,
        ProjectPath::new("oracle.yaml").unwrap(),
        &YamlLimits::default(),
    )
    .expect("parse YAML file")
    .value
}

pub fn suite_file(fixture: &str, name: &str) -> PathBuf {
    fixture_root(fixture)
        .parent()
        .expect("fixture suite root")
        .join(name)
}

pub fn mutate_object(path: &Path, mutation: impl FnOnce(&mut Value)) {
    let bytes = fs::read(path).expect("read mutation target");
    let mut object = parse_restricted_yaml(
        &bytes,
        ProjectPath::new("mutation-target.yaml").unwrap(),
        &YamlLimits::default(),
    )
    .expect("parse mutation target")
    .value;
    mutation(&mut object);
    fs::write(
        path,
        serde_json::to_vec_pretty(&object).expect("serialize mutation target"),
    )
    .expect("write mutation target");
}

pub fn materialize_overlay(fixture: &str, overlay: &str) -> (TempProject, &'static str) {
    let project = TempProject::copy_fixture(fixture);
    let descriptor_path = fixture_root(fixture)
        .parent()
        .expect("fixture suite root")
        .join("mutations")
        .join(format!("{overlay}.yaml"));
    let descriptor_bytes = fs::read(&descriptor_path).expect("read overlay descriptor");
    let descriptor = parse_restricted_yaml(
        &descriptor_bytes,
        ProjectPath::new("overlay.yaml").unwrap(),
        &YamlLimits::default(),
    )
    .expect("parse overlay descriptor")
    .value;
    let expected_code = descriptor
        .pointer("/expected_failure/error_code")
        .and_then(Value::as_str)
        .expect("overlay expected code");
    let expected_code: &'static str = match expected_code {
        "RP_E_ACCESS_ASSESSMENT_SUPERSESSION" => "RP_E_ACCESS_ASSESSMENT_SUPERSESSION",
        "RP_E_ACCESS_ASSESSMENT_TARGET" => "RP_E_ACCESS_ASSESSMENT_TARGET",
        "RP_E_ACCESS_BINDING_TARGET" => "RP_E_ACCESS_BINDING_TARGET",
        "RP_E_ACCESS_CLAIM_SELECTION" => "RP_E_ACCESS_CLAIM_SELECTION",
        "RP_E_ACCESS_COMPARTMENT_MISSING" => "RP_E_ACCESS_COMPARTMENT_MISSING",
        "RP_E_DECLASSIFICATION_UNSUPPORTED" => "RP_E_DECLASSIFICATION_UNSUPPORTED",
        "RP_E_ACCESS_LEVEL_DOWNGRADE" => "RP_E_ACCESS_LEVEL_DOWNGRADE",
        "RP_E_ACCESS_STRUCTURAL_ENDPOINT" => "RP_E_ACCESS_STRUCTURAL_ENDPOINT",
        "RP_E_ACCESS_RELATION_PARENT" => "RP_E_ACCESS_RELATION_PARENT",
        "RP_E_ACCESS_REVISION_PARENT" => "RP_E_ACCESS_REVISION_PARENT",
        "RP_E_ACCESS_THREAD_PARENT" => "RP_E_ACCESS_THREAD_PARENT",
        "RP_E_ACCESS_THREAD_ROOT" => "RP_E_ACCESS_THREAD_ROOT",
        "RP_E_ACCESS_LEVEL_UNKNOWN" => "RP_E_ACCESS_LEVEL_UNKNOWN",
        "RP_E_ACCESS_COMPARTMENTS_UNSORTED" => "RP_E_ACCESS_COMPARTMENTS_UNSORTED",
        "RP_E_CLAIM_CHAIN_DIRECTED_CYCLE" => "RP_E_CLAIM_CHAIN_DIRECTED_CYCLE",
        "RP_E_CLAIM_CHAIN_REQUIRES_EXACT_REVISION" => "RP_E_CLAIM_CHAIN_REQUIRES_EXACT_REVISION",
        "RP_E_CLAIM_CHAIN_MULTIPLE_COMPONENTS" => "RP_E_CLAIM_CHAIN_MULTIPLE_COMPONENTS",
        "RP_E_CLAIM_CHAIN_RELATION_NOT_ACTIVE" => "RP_E_CLAIM_CHAIN_RELATION_NOT_ACTIVE",
        "RP_E_CLAIM_CHAIN_ENDPOINT_NOT_SELECTED" => "RP_E_CLAIM_CHAIN_ENDPOINT_NOT_SELECTED",
        "RP_E_RELATION_KIND_INCOMPATIBLE" => "RP_E_RELATION_KIND_INCOMPATIBLE",
        "RP_E_EVIDENTIAL_TARGET_PATH_MISSING" => "RP_E_EVIDENTIAL_TARGET_PATH_MISSING",
        "RP_E_EVIDENTIAL_SYNTHESIS_REQUIRED" => "RP_E_EVIDENTIAL_SYNTHESIS_REQUIRED",
        "RP_E_EVIDENTIAL_RELATION_NOT_ALLOWED" => "RP_E_EVIDENTIAL_RELATION_NOT_ALLOWED",
        "RP_E_CONFIRMATORY_ROOT_KIND" => "RP_E_CONFIRMATORY_ROOT_KIND",
        "RP_E_CONFIRMATORY_BACKBONE_MISSING" => "RP_E_CONFIRMATORY_BACKBONE_MISSING",
        "RP_E_CONFIRMATORY_ARTIFACT_REQUIRED" => "RP_E_CONFIRMATORY_ARTIFACT_REQUIRED",
        "RP_E_CONFIRMATORY_RESULT_TEST_REQUIRED" => "RP_E_CONFIRMATORY_RESULT_TEST_REQUIRED",
        "RP_E_CONFIRMATORY_PREDECLARATION_ORDER" => "RP_E_CONFIRMATORY_PREDECLARATION_ORDER",
        other => panic!("unregistered test overlay code {other}"),
    };

    let mutation = descriptor.get("mutation").expect("overlay mutation");
    let target = mutation
        .get("target_file")
        .and_then(Value::as_str)
        .expect("overlay target file")
        .strip_prefix(".research/")
        .expect("project-relative overlay target");
    let target_path = project.research(target);
    let operation = mutation
        .get("operation")
        .and_then(Value::as_str)
        .expect("overlay operation");
    assert_ne!(operation, "text_replace", "text overlays are not used here");
    let object_bytes = fs::read(&target_path).expect("read overlay target");
    let mut object = parse_restricted_yaml(
        &object_bytes,
        ProjectPath::new(target.to_string()).unwrap(),
        &YamlLimits::default(),
    )
    .expect("parse overlay target")
    .value;
    let pointer = mutation
        .get("path")
        .and_then(Value::as_str)
        .expect("structured overlay pointer");
    match operation {
        "set_value" | "add_field" => set_json_pointer(
            &mut object,
            pointer,
            mutation.get("value").cloned().expect("overlay value"),
        ),
        "delete_field" => delete_json_pointer(&mut object, pointer),
        other => panic!("unsupported overlay operation {other}"),
    }
    fs::write(
        target_path,
        serde_json::to_vec_pretty(&object).expect("serialize mutated object"),
    )
    .expect("write overlay target");
    (project, expected_code)
}

pub fn replace(path: &Path, from: &str, to: &str) {
    let contents = fs::read_to_string(path).expect("read fixture file");
    assert!(
        contents.contains(from),
        "fixture did not contain mutation source"
    );
    fs::write(path, contents.replacen(from, to, 1)).expect("write fixture mutation");
}

fn set_json_pointer(root: &mut Value, pointer: &str, value: Value) {
    let (parent, token) = pointer.rsplit_once('/').expect("non-root JSON pointer");
    let parent = if parent.is_empty() {
        root
    } else {
        root.pointer_mut(parent).expect("overlay parent pointer")
    };
    let token = token.replace("~1", "/").replace("~0", "~");
    match parent {
        Value::Object(object) => {
            object.insert(token, value);
        }
        Value::Array(array) => {
            let index: usize = token.parse().expect("overlay array index");
            array[index] = value;
        }
        _ => panic!("overlay parent is not a collection"),
    }
}

fn delete_json_pointer(root: &mut Value, pointer: &str) {
    let (parent, token) = pointer.rsplit_once('/').expect("non-root JSON pointer");
    let parent = if parent.is_empty() {
        root
    } else {
        root.pointer_mut(parent).expect("overlay parent pointer")
    };
    let token = token.replace("~1", "/").replace("~0", "~");
    match parent {
        Value::Object(object) => {
            object.remove(&token).expect("overlay field exists");
        }
        Value::Array(array) => {
            let index: usize = token.parse().expect("overlay array index");
            array.remove(index);
        }
        _ => panic!("overlay parent is not a collection"),
    }
}

fn unique_temp_path() -> PathBuf {
    let counter = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("rp-phase1a-{}-{counter}", std::process::id()))
}

fn copy_tree(source: &Path, target: &Path) {
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let file_type = entry.file_type().expect("read fixture entry type");
        let destination = target.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&destination).expect("create copied fixture directory");
            copy_tree(&entry.path(), &destination);
        } else if file_type.is_file() {
            fs::copy(entry.path(), destination).expect("copy fixture file");
        } else {
            panic!("fixtures must contain only regular files and directories");
        }
    }
}
