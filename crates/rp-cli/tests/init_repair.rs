use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use rp_core::{ProjectPath, SchemaBundle};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rp-cli-init-repair-{}-{}",
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

fn init(root: &Path, cwd: &Path, expected_exit: i32) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_rp"))
        .args(["init", "--project"]).arg(root).arg("--json")
        .current_dir(cwd)
        // No git (or any other child executable) can be discovered via PATH.
        .env("PATH", "")
        .output().unwrap();
    assert_eq!(output.status.code(), Some(expected_exit), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["command"], "init");
    assert_eq!(result["exit_code"], expected_exit);
    SchemaBundle::new()
        .unwrap()
        .validate(&result, ProjectPath::new("result.json").unwrap())
        .unwrap_or_else(|f| panic!("invalid envelope: {f:?}"));
    result
}

#[test]
fn cli_initializes_existing_git_directory_or_worktree_without_git_and_preserves_conflict_contract()
{
    for git_directory in [true, false] {
        let repo = Temp::new();
        let git = repo.0.join(".git");
        if git_directory {
            fs::create_dir(&git).unwrap();
            fs::write(git.join("config"), b"git sentinel").unwrap();
        } else {
            fs::write(&git, b"gitdir: /not-followed/worktree\n").unwrap();
        }
        fs::write(repo.0.join("keep"), b"sentinel").unwrap();
        fs::set_permissions(&repo.0, fs::Permissions::from_mode(0o751)).unwrap();
        let result = init(Path::new("."), &repo.0, 0);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["data"]["schema"], "rp/cli-data/init/v1");
        let paths = result["data"]["created_paths"].as_array().unwrap();
        assert_eq!(paths.len(), 29);
        assert!(
            paths
                .iter()
                .all(|p| p.as_str().unwrap().starts_with(".research"))
        );
        let descriptor = fs::read(repo.0.join(".research/project.yaml")).unwrap();
        let conflict = init(Path::new("."), &repo.0, 1);
        assert_eq!(conflict["status"], "conflict");
        assert_eq!(
            conflict["findings"][0]["error_code"],
            "RP_E_INIT_PROJECT_EXISTS"
        );
        assert!(conflict["data"].is_null());
        assert_eq!(
            fs::read(repo.0.join(".research/project.yaml")).unwrap(),
            descriptor
        );
        assert_eq!(fs::read(repo.0.join("keep")).unwrap(), b"sentinel");
        assert_eq!(
            fs::metadata(&repo.0).unwrap().permissions().mode() & 0o7777,
            0o751
        );
        assert_eq!(fs::read_dir(&repo.0).unwrap().count(), 3);
        assert_eq!(
            fs::read(if git_directory {
                git.join("config")
            } else {
                git
            })
            .unwrap(),
            if git_directory {
                &b"git sentinel"[..]
            } else {
                &b"gitdir: /not-followed/worktree\n"[..]
            }
        );
    }
}

#[test]
fn cli_research_symlinks_are_conflicts_but_root_symlinks_are_io_errors() {
    let parent = Temp::new();
    let outside = Temp::new();
    for dangling in [false, true] {
        let root = parent
            .0
            .join(if dangling { "dangling-repo" } else { "repo" });
        fs::create_dir(&root).unwrap();
        let target = if dangling {
            outside.0.join("missing")
        } else {
            outside.0.clone()
        };
        symlink(&target, root.join(".research")).unwrap();
        let result = init(&root, &parent.0, 1);
        assert_eq!(result["status"], "conflict");
        assert_eq!(
            result["findings"][0]["error_code"],
            "RP_E_INIT_PROJECT_EXISTS"
        );
        assert_eq!(fs::read_link(root.join(".research")).unwrap(), target);
        let link = parent.0.join(if dangling {
            "dangling-root-link"
        } else {
            "root-link"
        });
        symlink(&target, &link).unwrap();
        let result = init(&link, &parent.0, 3);
        assert_eq!(result["status"], "io-error");
        assert_eq!(result["findings"][0]["error_code"], "RP_E_IO_READ_FAILED");
    }
    assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 0);
}
