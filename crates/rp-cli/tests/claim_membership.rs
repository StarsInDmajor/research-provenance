#[path = "../../rp-core/tests/common/mod.rs"]
mod common;

use common::{TempProject, mutate_object};
use rp_core::{ProjectPath, SchemaBundle};
use serde_json::Value;
use std::process::Command;

#[test]
fn missing_membership_is_stage_three_invalid_in_exact_cli_envelopes() {
    let schema = SchemaBundle::new().unwrap();
    let chain_id = "cch_01J00000000000000000000051";
    let source =
        ".research/claim-chains/minimum-structural-chain--cch_01J00000000000000000000051.yaml";
    for profile in ["minimum-v1", "evidential-v1", "confirmatory-v1"] {
        for (root, target) in [(true, false), (false, true), (true, true)] {
            let project = TempProject::copy_fixture("claim-chain-minimum-v1");
            mutate_object(&project.path().join(source), |chain| {
                chain["validation_policy"]["profile"] = profile.into();
                let unselected = "qst_01J00000000000000000000020";
                if root {
                    chain["root_node_revisions"][0] = unselected.into();
                }
                if target {
                    chain["target_node_revision"] = unselected.into();
                }
                // Only roots/target/profile change, not the selected source closure.
            });
            for chain_command in [false, true] {
                let mut command = Command::new(env!("CARGO_BIN_EXE_rp"));
                if chain_command {
                    command.args(["chain", "validate", chain_id, "--profile", profile]);
                } else {
                    command.arg("validate");
                }
                let output = command
                    .arg("--project")
                    .arg(project.path())
                    .arg("--json")
                    .output()
                    .unwrap();
                assert_eq!(output.status.code(), Some(1));
                assert!(output.stderr.is_empty());
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                schema
                    .validate(&value, ProjectPath::new("cli-result.json").unwrap())
                    .unwrap();
                assert_eq!(value["status"], "invalid");
                assert_eq!(value["exit_code"], 1);
                assert_eq!(value["data"]["valid"], false);
                if !chain_command {
                    assert_eq!(value["data"]["stage3_ran"], true);
                }
                let pointers: Vec<_> = value["findings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|finding| {
                        assert_eq!(finding["schema"], "rp/finding/v1");
                        assert_eq!(
                            finding["error_code"],
                            "RP_E_CLAIM_CHAIN_ENDPOINT_NOT_SELECTED"
                        );
                        assert_eq!(finding["finding_family"], "endpoint_containment");
                        assert_eq!(finding["severity"], "error");
                        assert_eq!(finding["source_file"], source);
                        finding["json_pointer"].as_str().unwrap()
                    })
                    .collect();
                let mut expected = Vec::new();
                if root {
                    expected.push("/root_node_revisions/0");
                }
                if target {
                    expected.push("/target_node_revision");
                }
                assert_eq!(pointers, expected);
            }
        }
    }
}
