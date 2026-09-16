mod common;

use std::{fs, os::unix::fs::PermissionsExt};

use common::TempProject;
use rp_core::{InitError, initialize_project, validate_project};

#[test]
fn init_creates_a_private_valid_skeleton_without_layer_a() {
    let parent = TempProject::empty();
    let root = parent.path().join("new-research-project");
    let report = initialize_project(&root).expect("initialize project");
    assert!(
        report
            .created_paths
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
    assert_eq!(report.created_paths.len(), 29);
    for path in &report.created_paths {
        let metadata = fs::metadata(root.join(path)).unwrap();
        assert_eq!(
            metadata.permissions().mode() & 0o7777,
            if metadata.is_dir() { 0o700 } else { 0o600 }
        );
    }
    assert!(root.join(".research/project.yaml").is_file());
    assert!(root.join(".research/records/questions").is_dir());
    assert!(root.join(".research/claim-chains").is_dir());
    assert!(!root.join(".research/events").exists());
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(root.join(".research/project.yaml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let validation = validate_project(&root).unwrap();
    assert!(validation.findings.is_empty(), "{:?}", validation.findings);
}

#[test]
fn init_descriptor_is_valid_even_when_directory_name_is_a_yaml_scalar() {
    let parent = TempProject::empty();
    for name in ["123", "true", "null", "FALSE", "---", "研究"] {
        let root = parent.path().join(name);
        let report = initialize_project(&root).unwrap();
        let descriptor = common::parse_yaml_file(&root.join(".research/project.yaml"));
        assert_eq!(descriptor["id"], report.project_id);
        let validation = validate_project(&root).unwrap();
        assert!(
            validation.findings.is_empty(),
            "{name}: {:?}",
            validation.findings
        );
    }
}

#[test]
fn init_never_overwrites_an_existing_project() {
    let parent = TempProject::empty();
    let existing = parent.path().join("existing");
    initialize_project(&existing).unwrap();
    assert!(matches!(
        initialize_project(&existing),
        Err(InitError::Conflict)
    ));
}

#[test]
fn init_existing_repository_preserves_every_existing_entry_and_mode() {
    use std::os::unix::fs::symlink;
    for git_directory in [true, false] {
        let repo = TempProject::empty();
        fs::set_permissions(repo.path(), fs::Permissions::from_mode(0o751)).unwrap();
        let git = repo.path().join(".git");
        if git_directory {
            fs::create_dir(&git).unwrap();
            fs::write(git.join("config"), b"[core]\n  bare = false\n").unwrap();
        } else {
            fs::write(&git, b"gitdir: /not-accessed/worktrees/example\n").unwrap();
        }
        fs::set_permissions(&git, fs::Permissions::from_mode(0o750)).unwrap();
        fs::write(repo.path().join("keep.txt"), b"unrelated\0bytes\xff").unwrap();
        fs::set_permissions(
            repo.path().join("keep.txt"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        symlink("absent-target", repo.path().join("unrelated-link")).unwrap();
        let report =
            initialize_project(repo.path()).expect("initialize ordinary existing repository");
        assert_eq!(
            fs::metadata(repo.path()).unwrap().permissions().mode() & 0o7777,
            0o751
        );
        assert_eq!(
            fs::metadata(&git).unwrap().permissions().mode() & 0o7777,
            0o750
        );
        assert_eq!(
            fs::metadata(repo.path().join("keep.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o640
        );
        assert_eq!(
            fs::read(repo.path().join("keep.txt")).unwrap(),
            b"unrelated\0bytes\xff"
        );
        assert_eq!(
            fs::read_link(repo.path().join("unrelated-link")).unwrap(),
            std::path::Path::new("absent-target")
        );
        assert_eq!(
            fs::read(if git_directory {
                git.join("config")
            } else {
                git
            })
            .unwrap(),
            if git_directory {
                &b"[core]\n  bare = false\n"[..]
            } else {
                &b"gitdir: /not-accessed/worktrees/example\n"[..]
            }
        );
        assert_eq!(fs::read_dir(repo.path()).unwrap().count(), 4);
        let descriptor = common::parse_yaml_file(&repo.research("project.yaml"));
        assert_eq!(descriptor["id"], report.project_id);
        assert!(validate_project(repo.path()).unwrap().findings.is_empty());
    }
}

#[test]
fn init_existing_empty_root_keeps_permissions_and_does_not_create_git() {
    let repo = TempProject::empty();
    fs::set_permissions(repo.path(), fs::Permissions::from_mode(0o755)).unwrap();
    initialize_project(repo.path()).unwrap();
    assert_eq!(
        fs::metadata(repo.path()).unwrap().permissions().mode() & 0o7777,
        0o755
    );
    assert_eq!(fs::read_dir(repo.path()).unwrap().count(), 1);
}

#[test]
fn init_research_conflicts_include_files_directories_and_all_symlinks() {
    use std::os::unix::fs::symlink;
    for kind in ["file", "directory", "symlink", "dangling"] {
        let repo = TempProject::empty();
        let outside = TempProject::empty();
        fs::set_permissions(repo.path(), fs::Permissions::from_mode(0o751)).unwrap();
        let research = repo.path().join(".research");
        match kind {
            "file" => fs::write(&research, b"existing").unwrap(),
            "directory" => {
                fs::create_dir(&research).unwrap();
                fs::write(research.join("keep"), b"existing").unwrap();
            }
            "symlink" => symlink(outside.path(), &research).unwrap(),
            _ => symlink(outside.path().join("missing"), &research).unwrap(),
        }
        let before = fs::symlink_metadata(&research).unwrap();
        assert!(
            matches!(initialize_project(repo.path()), Err(InitError::Conflict)),
            "{kind}"
        );
        assert_eq!(
            fs::symlink_metadata(&research)
                .unwrap()
                .permissions()
                .mode(),
            before.permissions().mode()
        );
        assert_eq!(
            fs::metadata(repo.path()).unwrap().permissions().mode() & 0o7777,
            0o751
        );
        assert_eq!(fs::read_dir(repo.path()).unwrap().count(), 1);
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        if kind == "file" {
            assert_eq!(fs::read(&research).unwrap(), b"existing");
        }
        if kind == "directory" {
            assert_eq!(fs::read(research.join("keep")).unwrap(), b"existing");
        }
    }
}

#[test]
fn init_rejects_root_and_ancestor_symlinks_without_touching_targets() {
    use std::os::unix::fs::symlink;
    let parent = TempProject::empty();
    let outside = TempProject::empty();
    fs::set_permissions(outside.path(), fs::Permissions::from_mode(0o751)).unwrap();
    symlink(outside.path(), parent.path().join("link")).unwrap();
    symlink(
        outside.path().join("missing"),
        parent.path().join("dangling"),
    )
    .unwrap();
    for path in [
        parent.path().join("link"),
        parent.path().join("link/new"),
        parent.path().join("dangling"),
        parent.path().join("link/../escape"),
    ] {
        assert!(initialize_project(&path).is_err(), "{}", path.display());
    }
    assert_eq!(
        fs::metadata(outside.path()).unwrap().permissions().mode() & 0o7777,
        0o751
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 2);
}

#[test]
fn init_does_not_create_missing_ancestors_or_accept_regular_root() {
    let parent = TempProject::empty();
    assert!(initialize_project(&parent.path().join("missing/child")).is_err());
    assert!(!parent.path().join("missing").exists());
    fs::write(parent.path().join("file"), b"keep").unwrap();
    assert!(initialize_project(&parent.path().join("file")).is_err());
    assert_eq!(fs::read(parent.path().join("file")).unwrap(), b"keep");
}

#[test]
fn init_concurrent_contenders_have_exactly_one_winner() {
    for missing_root in [false, true] {
        concurrent_init(missing_root);
    }
}

fn concurrent_init(missing_root: bool) {
    use std::sync::{Arc, Barrier};
    let repo = TempProject::empty();
    let root = if missing_root {
        repo.path().join("new")
    } else {
        repo.path().to_path_buf()
    };
    let barrier = Arc::new(Barrier::new(2));
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let root = root.as_path();
                scope.spawn(move || {
                    barrier.wait();
                    initialize_project(root)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(InitError::Conflict)))
            .count(),
        1
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    let winner = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(
        common::parse_yaml_file(&root.join(".research/project.yaml"))["id"],
        winner.project_id
    );
    assert!(validate_project(&root).unwrap().findings.is_empty());
}
