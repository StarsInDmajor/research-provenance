use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rp-init-hooks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn execution_stop_before_publish_cleans_staging_after_publish_keeps_tree() {
    use std::sync::{Arc, atomic::AtomicBool};
    for step in [
        Step::Staged,
        Step::DirectoryCreated,
        Step::PartialWrite,
        Step::BeforePublish,
        Step::PublishChecked,
        Step::Published,
    ] {
        let parent = Temp::new();
        let token = Arc::new(AtomicBool::new(false));
        let budget = crate::ExecutionBudget::new(std::time::Duration::from_secs(60), token.clone());
        let result = initialize_linux_with_budget(
            &parent.0,
            &mut |observed| {
                if observed == step {
                    token.store(true, Ordering::Release);
                }
                Ok(())
            },
            &budget,
        );
        assert!(matches!(
            result,
            Err(InitError::Stopped(crate::ExecutionStop::Interrupted))
        ));
        assert_eq!(
            parent.0.join(".research/project.yaml").exists(),
            step == Step::Published
        );
        assert_eq!(
            fs::read_dir(&parent.0).unwrap().count(),
            usize::from(step == Step::Published)
        );
    }
}

#[test]
fn failures_before_publish_clean_only_owned_staging() {
    for step in [
        Step::Staged,
        Step::DirectoryCreated,
        Step::PartialWrite,
        Step::FileSync,
        Step::DirectorySync,
        Step::BeforePublish,
    ] {
        for existing in [false, true] {
            let parent = Temp::new();
            let root = parent.0.join("repo");
            if existing {
                fs::create_dir(&root).unwrap();
                fs::set_permissions(&root, fs::Permissions::from_mode(0o751)).unwrap();
                fs::write(root.join("sentinel"), b"keep").unwrap();
                fs::create_dir(root.join(".rp-init-preexisting")).unwrap();
                fs::write(root.join(".rp-init-preexisting/keep"), b"not owned").unwrap();
            }
            let mut reached = false;
            let result = initialize_linux(&root, &mut |observed| {
                if observed == step {
                    reached = true;
                    return Err(std::io::Error::other(
                        "injected write/sync/prepublish failure",
                    ));
                }
                Ok(())
            });
            assert!(reached, "{step:?}");
            assert!(matches!(result, Err(InitError::Io(_))), "{result:?}");
            assert!(!root.join(".research").exists());
            assert_eq!(
                fs::read_dir(&root).unwrap().count(),
                if existing { 2 } else { 0 }
            );
            assert_eq!(
                fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                if existing { 0o751 } else { 0o700 }
            );
            if existing {
                assert_eq!(fs::read(root.join("sentinel")).unwrap(), b"keep");
                assert_eq!(
                    fs::read(root.join(".rp-init-preexisting/keep")).unwrap(),
                    b"not owned"
                );
            }
        }
    }
}

#[test]
fn deterministic_root_and_ancestor_substitution_never_publishes_to_foreign_root() {
    for ancestor_swap in [false, true] {
        for replace_with_symlink in [false, true] {
            let parent = Temp::new();
            let ancestor = parent.0.join("ancestor");
            let root = ancestor.join("repo");
            fs::create_dir_all(&root).unwrap();
            let foreign = parent.0.join("foreign");
            fs::create_dir(&foreign).unwrap();
            fs::write(foreign.join("sentinel"), b"foreign").unwrap();
            let moved = parent.0.join("moved");
            let result = initialize_linux(&root, &mut |step| {
                if step == Step::BeforePublish {
                    let target = if ancestor_swap { &ancestor } else { &root };
                    fs::rename(target, &moved).unwrap();
                    if replace_with_symlink {
                        symlink(&foreign, target).unwrap();
                    } else {
                        fs::create_dir(target).unwrap();
                        if ancestor_swap {
                            fs::create_dir(target.join("repo")).unwrap();
                        }
                    }
                }
                Ok(())
            });
            assert!(matches!(result, Err(InitError::Io(_))));
            assert!(!root.join(".research").exists());
            assert_eq!(
                fs::read_dir(if ancestor_swap {
                    moved.join("repo")
                } else {
                    moved
                })
                .unwrap()
                .count(),
                0
            );
            assert_eq!(fs::read_dir(&foreign).unwrap().count(), 1);
            assert_eq!(fs::read(foreign.join("sentinel")).unwrap(), b"foreign");
        }
    }
}

#[test]
fn publication_race_conflicts_without_overwriting_any_entry_type() {
    for kind in ["file", "directory", "symlink", "dangling"] {
        let root = Temp::new();
        let foreign = Temp::new();
        let research = root.0.join(".research");
        let result = initialize_linux(&root.0, &mut |step| {
            if step == Step::BeforePublish {
                match kind {
                    "file" => fs::write(&research, b"winner").unwrap(),
                    "directory" => fs::create_dir(&research).unwrap(),
                    "symlink" => symlink(&foreign.0, &research).unwrap(),
                    _ => symlink(foreign.0.join("missing"), &research).unwrap(),
                }
            }
            Ok(())
        });
        assert!(
            matches!(result, Err(InitError::Conflict)),
            "{kind}: {result:?}"
        );
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&foreign.0).unwrap().count(), 0);
        if kind == "file" {
            assert_eq!(fs::read(&research).unwrap(), b"winner");
        }
        if kind == "directory" {
            assert_eq!(fs::read_dir(&research).unwrap().count(), 0);
        }
        if kind == "symlink" || kind == "dangling" {
            assert!(fs::symlink_metadata(&research).unwrap().is_symlink());
        }
    }
}

#[test]
fn cleanup_uses_pinned_staging_not_a_same_name_replacement() {
    let root = Temp::new();
    let mut moved = None;
    let result = initialize_linux(&root.0, &mut |step| {
        if step == Step::PartialWrite {
            let stage = fs::read_dir(&root.0)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let saved = root.0.join("moved-stage");
            fs::rename(&stage, &saved).unwrap();
            fs::create_dir(&stage).unwrap();
            fs::write(stage.join("sentinel"), b"foreign").unwrap();
            moved = Some(saved);
            return Err(std::io::Error::other(
                "injected failure after stage replacement",
            ));
        }
        Ok(())
    });
    assert!(matches!(result, Err(InitError::Io(_))));
    assert!(!root.0.join(".research").exists());
    assert_eq!(fs::read_dir(moved.unwrap()).unwrap().count(), 0);
    let foreign = fs::read_dir(&root.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap() != "moved-stage")
        .unwrap();
    assert_eq!(fs::read(foreign.join("sentinel")).unwrap(), b"foreign");
}

#[test]
fn rename_after_identity_check_remains_bound_to_the_open_root() {
    let parent = Temp::new();
    let root = parent.0.join("repo");
    let moved = parent.0.join("renamed");
    fs::create_dir(&root).unwrap();
    initialize_linux(&root, &mut |step| {
        if step == Step::PublishChecked {
            fs::rename(&root, &moved).unwrap();
            fs::create_dir(&root).unwrap();
            fs::write(root.join("sentinel"), b"foreign").unwrap();
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    assert!(moved.join(".research/project.yaml").is_file());
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 1);
}
