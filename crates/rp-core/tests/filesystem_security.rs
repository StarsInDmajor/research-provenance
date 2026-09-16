mod common;

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        fs,
        os::unix::{fs::symlink, net::UnixListener},
    };

    use super::common::TempProject;
    use rp_core::{ProjectLimits, ProjectPath, validate_project, validate_project_with_limits};

    #[test]
    fn canonical_file_symlinks_are_rejected_without_being_followed() {
        let project = TempProject::copy_fixture("freshness-v1");
        let outside = project.path().join("outside.yaml");
        fs::write(&outside, b"not: a canonical object\n").expect("write outside file");
        symlink(
            &outside,
            project.research("records/questions/escape--qst_01J00000000000000000000199.yaml"),
        )
        .expect("create file symlink");

        let report = validate_project(project.path()).expect("scan project");
        assert!(!report.stage3_ran);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.error_code == "RP_E_PATH_SYMLINK_FORBIDDEN")
        );
    }

    #[test]
    fn directory_symlinks_are_rejected_and_never_descended() {
        let project = TempProject::copy_fixture("freshness-v1");
        let outside = project.path().join("outside");
        fs::create_dir(&outside).expect("create outside directory");
        fs::write(outside.join("secret.yaml"), b"secret: must-not-be-read\n")
            .expect("write outside file");
        symlink(&outside, project.research("records/escaped")).expect("create directory symlink");

        let report = validate_project(project.path()).expect("scan project");
        assert!(!report.stage3_ran);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.error_code == "RP_E_PATH_SYMLINK_FORBIDDEN")
        );
        assert!(
            !report
                .canonical_paths
                .iter()
                .any(|path| path.as_str().contains("secret.yaml"))
        );
    }

    #[test]
    fn non_regular_canonical_entries_are_rejected() {
        let project = TempProject::copy_fixture("freshness-v1");
        let socket =
            project.research("records/questions/socket--qst_01J00000000000000000000199.yaml");
        let _listener = UnixListener::bind(&socket).expect("create Unix socket");

        let report = validate_project(project.path()).expect("scan project");
        assert!(!report.stage3_ran);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.error_code == "RP_E_PATH_NON_REGULAR_FILE")
        );
    }

    #[test]
    fn oversized_canonical_files_cannot_bypass_the_scan_limit() {
        let project = TempProject::empty();
        fs::create_dir_all(project.research("threads")).expect("create research directories");
        for name in [
            "threads/a.yaml",
            "threads/b.yaml",
            "threads/c.yaml",
            "project.yaml",
        ] {
            fs::write(project.research(name), b"oversized canonical object")
                .expect("write oversized object");
        }
        let report = validate_project_with_limits(
            project.path(),
            ProjectLimits {
                canonical_files: 2,
                yaml_bytes_per_object: 4,
                ..ProjectLimits::default()
            },
        )
        .expect("bounded oversized scan");
        assert!(
            report.findings.iter().any(|finding| {
                finding.error_code == "RP_E_RESOURCE_SCANNED_FILES_EXCEEDED"
                    && finding.source_file.as_ref().unwrap().as_str() == ".research/threads/b.yaml"
            }),
            "{:?}",
            report.findings
        );
        assert_eq!(report.canonical_object_count, 0);
        assert!(report.canonical_paths.is_empty());
        assert!(!report.stage3_ran);
    }

    #[test]
    fn scan_limits_skip_parsing_and_byte_accounting_for_partial_scans() {
        for (limits, expected_code) in [
            (
                ProjectLimits {
                    canonical_files: 1,
                    ..ProjectLimits::default()
                },
                "RP_E_RESOURCE_SCANNED_FILES_EXCEEDED",
            ),
            (
                ProjectLimits {
                    scanned_directories: 2,
                    ..ProjectLimits::default()
                },
                "RP_E_RESOURCE_SCANNED_DIRECTORIES_EXCEEDED",
            ),
        ] {
            let project = TempProject::empty();
            fs::create_dir_all(project.research("threads")).expect("create research directories");
            fs::write(project.research("project.yaml"), b"broken: [\n")
                .expect("write malformed descriptor");
            fs::write(project.research("threads/z.yaml"), b"broken: [\n")
                .expect("write excess object");
            fs::create_dir(project.research("threads/zz-directory"))
                .expect("create excess directory");
            for whole_project_yaml_bytes in [1, usize::MAX] {
                let report = validate_project_with_limits(
                    project.path(),
                    ProjectLimits {
                        whole_project_yaml_bytes,
                        ..limits
                    },
                )
                .expect("bounded partial scan");
                let codes: Vec<_> = report
                    .findings
                    .iter()
                    .map(|finding| finding.error_code)
                    .collect();
                assert_eq!(codes, [expected_code]);
                assert_eq!(report.canonical_object_count, 0);
                assert!(report.canonical_paths.is_empty());
                assert!(!report.stage3_ran);
                assert!(report.index.is_none());
            }
        }
    }

    #[test]
    fn normalized_paths_reject_raw_and_percent_encoded_traversal() {
        for path in [
            "../secret",
            "%2e%2e/secret",
            "%2E%2E%2Fsecret",
            "safe/%2e%2e/secret",
            "safe/%2fetc/passwd",
            "safe/%5c..%5csecret",
        ] {
            assert!(ProjectPath::new(path).is_err(), "accepted {path}");
        }
    }
}
