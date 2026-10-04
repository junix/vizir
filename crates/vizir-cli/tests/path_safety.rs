use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SENTINEL: &[u8] = b"existing artifact must survive a rejected command\n";

fn input(directory: &Path) -> PathBuf {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chart/service-health.viz.yaml");
    let input = directory.join("source.viz.yaml");
    fs::copy(source, &input).unwrap();
    input
}

fn run(kind: &str, input: &Path, output: &Path, manifest: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
    if matches!(kind, "svg" | "png") {
        command.args(["render", "--format", kind]);
    } else {
        command.arg(kind);
    }
    command.arg(input).arg("--output").arg(output);
    if let Some(manifest) = manifest {
        command.arg("--manifest").arg(manifest);
    }
    if kind == "png" {
        // Rejections must happen before probing or invoking a rasterizer.
        command.env("PATH", "");
    }
    command.output().unwrap()
}

fn rejected(result: Output, code: &str, roles: &[&str]) {
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(code), "{stderr}");
    for role in roles {
        assert!(stderr.contains(&format!("{role} path")), "{stderr}");
    }
    assert!(result.stdout.is_empty(), "{:?}", result.stdout);
}

fn accepted(result: Output) {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn every_writing_command_rejects_the_input_as_output_without_changing_it() {
    for kind in ["normalize", "lower", "svg", "png"] {
        for spelling in [
            "source.viz.yaml",
            "./source.viz.yaml",
            "sub/../source.viz.yaml",
        ] {
            let temporary = tempfile::tempdir().unwrap();
            let source = input(temporary.path());
            let original = fs::read(&source).unwrap();
            fs::create_dir(temporary.path().join("sub")).unwrap();
            rejected(
                run(kind, &source, &temporary.path().join(spelling), None),
                "VIZ-PATH-0001",
                &["input", "output"],
            );
            assert_eq!(fs::read(&source).unwrap(), original, "{kind}: {spelling}");
        }
    }
}

#[test]
fn relative_and_absolute_spellings_collide() {
    let temporary = tempfile::tempdir().unwrap();
    let source = input(temporary.path());
    let original = fs::read(&source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_vizir"))
        .current_dir(temporary.path())
        .args(["normalize", "source.viz.yaml", "--output"])
        .arg(&source)
        .output()
        .unwrap();
    rejected(result, "VIZ-PATH-0001", &["input", "output"]);
    assert_eq!(fs::read(source).unwrap(), original);
}

#[test]
fn manifest_cannot_overwrite_source_and_render_never_starts() {
    for kind in ["svg", "png"] {
        for existing_output in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let source = input(temporary.path());
            let original = fs::read(&source).unwrap();
            let output = temporary.path().join("new/plot.svg");
            if existing_output {
                fs::create_dir(output.parent().unwrap()).unwrap();
                fs::write(&output, SENTINEL).unwrap();
            }
            rejected(
                run(kind, &source, &output, Some(&source)),
                "VIZ-PATH-0001",
                &["input", "manifest"],
            );
            assert_eq!(fs::read(source).unwrap(), original);
            if existing_output {
                assert_eq!(fs::read(output).unwrap(), SENTINEL);
            } else {
                assert!(!output.parent().unwrap().exists());
            }
        }
    }
}

#[test]
fn output_and_manifest_must_be_distinct_even_when_both_are_new() {
    for kind in ["svg", "png"] {
        for existing in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let source = input(temporary.path());
            let original = fs::read(&source).unwrap();
            let output = temporary.path().join("new/plot.svg");
            if existing {
                fs::create_dir(output.parent().unwrap()).unwrap();
                fs::write(&output, SENTINEL).unwrap();
            }
            let manifest = temporary.path().join("new/./plot.svg");
            rejected(
                run(kind, &source, &output, Some(&manifest)),
                "VIZ-PATH-0001",
                &["output", "manifest"],
            );
            assert_eq!(fs::read(source).unwrap(), original);
            if existing {
                assert_eq!(fs::read(output).unwrap(), SENTINEL);
            } else {
                assert!(!output.parent().unwrap().exists());
            }
        }
    }
}

#[test]
fn hardlinked_inputs_outputs_and_manifests_are_protected() {
    for kind in ["normalize", "lower", "svg", "png"] {
        let temporary = tempfile::tempdir().unwrap();
        let source = input(temporary.path());
        let original = fs::read(&source).unwrap();
        let alias = temporary.path().join("alias");
        fs::hard_link(&source, &alias).unwrap();
        rejected(
            run(kind, &source, &alias, None),
            "VIZ-PATH-0001",
            &["input", "output"],
        );
        assert_eq!(fs::read(&alias).unwrap(), original);
        if matches!(kind, "svg" | "png") {
            let output = temporary.path().join("plot");
            fs::write(&output, SENTINEL).unwrap();
            rejected(
                run(kind, &source, &output, Some(&alias)),
                "VIZ-PATH-0001",
                &["input", "manifest"],
            );
            assert_eq!(fs::read(&output).unwrap(), SENTINEL);
            let manifest = temporary.path().join("manifest");
            fs::hard_link(&output, &manifest).unwrap();
            rejected(
                run(kind, &source, &output, Some(&manifest)),
                "VIZ-PATH-0001",
                &["output", "manifest"],
            );
            assert_eq!(fs::read(output).unwrap(), SENTINEL);
            assert_eq!(fs::read(manifest).unwrap(), SENTINEL);
        }
        assert_eq!(fs::read(source).unwrap(), original);
    }
}

#[test]
fn missing_parent_then_dotdot_cannot_reach_an_input_hardlink() {
    let temporary = tempfile::tempdir().unwrap();
    let source = input(temporary.path());
    let original = fs::read(&source).unwrap();
    let alias = temporary.path().join("alias");
    fs::hard_link(&source, &alias).unwrap();
    let output = temporary.path().join("missing/../alias");
    rejected(
        run("normalize", &source, &output, None),
        "VIZ-PATH-0001",
        &["input", "output"],
    );
    assert_eq!(fs::read(source).unwrap(), original);
    assert_eq!(fs::read(alias).unwrap(), original);
    assert!(!temporary.path().join("missing").exists());
}

#[test]
fn distinct_nested_destinations_and_existing_artifacts_remain_supported() {
    for kind in ["normalize", "lower", "svg"] {
        let temporary = tempfile::tempdir().unwrap();
        let source = input(temporary.path());
        let original = fs::read(&source).unwrap();
        let output = temporary.path().join("new/deep/source.viz.yaml");
        accepted(run(kind, &source, &output, None));
        assert!(!fs::read(&output).unwrap().is_empty());
        fs::write(&output, SENTINEL).unwrap();
        accepted(run(kind, &source, &output, None));
        assert_ne!(fs::read(&output).unwrap(), SENTINEL);
        assert_eq!(fs::read(&source).unwrap(), original);
    }
    let temporary = tempfile::tempdir().unwrap();
    let source = input(temporary.path());
    let output = temporary.path().join("artifacts/plot");
    let manifest = temporary.path().join("reports/plot");
    accepted(run("svg", &source, &output, Some(&manifest)));
    assert!(fs::read_to_string(output).unwrap().starts_with("<svg"));
    let report: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    assert_eq!(report["format"], "svg");
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn symlinked_input_and_destinations_are_protected_in_every_role() {
        for kind in ["normalize", "lower", "svg", "png"] {
            let temporary = tempfile::tempdir().unwrap();
            let source = input(temporary.path());
            let original = fs::read(&source).unwrap();
            let alias = temporary.path().join("alias");
            symlink("source.viz.yaml", &alias).unwrap();
            rejected(
                run(kind, &source, &alias, None),
                "VIZ-PATH-0001",
                &["input", "output"],
            );
            rejected(
                run(kind, &alias, &source, None),
                "VIZ-PATH-0001",
                &["input", "output"],
            );
            if matches!(kind, "svg" | "png") {
                let output = temporary.path().join("plot");
                fs::write(&output, SENTINEL).unwrap();
                rejected(
                    run(kind, &source, &output, Some(&alias)),
                    "VIZ-PATH-0001",
                    &["input", "manifest"],
                );
                let manifest = temporary.path().join("manifest");
                symlink("plot", &manifest).unwrap();
                rejected(
                    run(kind, &source, &output, Some(&manifest)),
                    "VIZ-PATH-0001",
                    &["output", "manifest"],
                );
                assert_eq!(fs::read(output).unwrap(), SENTINEL);
                assert_eq!(fs::read(manifest).unwrap(), SENTINEL);
            }
            assert_eq!(fs::read(source).unwrap(), original);
            assert_eq!(fs::read(alias).unwrap(), original);
        }
    }

    #[test]
    fn symlinked_parents_and_dangling_links_cannot_alias_new_artifacts() {
        for kind in ["svg", "png"] {
            for dangling_leaf in [false, true] {
                let temporary = tempfile::tempdir().unwrap();
                let source = input(temporary.path());
                let original = fs::read(&source).unwrap();
                fs::create_dir(temporary.path().join("real")).unwrap();
                symlink("real", temporary.path().join("alias")).unwrap();
                let output = temporary.path().join("real/new/plot");
                let manifest = if dangling_leaf {
                    let path = temporary.path().join("manifest");
                    symlink("alias/new/plot", &path).unwrap();
                    path
                } else {
                    temporary.path().join("alias/new/plot")
                };
                rejected(
                    run(kind, &source, &output, Some(&manifest)),
                    "VIZ-PATH-0001",
                    &["output", "manifest"],
                );
                assert_eq!(fs::read(source).unwrap(), original);
                assert!(!output.exists());
                assert!(!temporary.path().join("real/new").exists());
            }
        }
    }

    #[test]
    fn symlink_dotdot_uses_the_filesystem_parent_and_preserves_distinct_paths() {
        let temporary = tempfile::tempdir().unwrap();
        fs::create_dir_all(temporary.path().join("real/sub")).unwrap();
        let source = input(&temporary.path().join("real"));
        let original = fs::read(&source).unwrap();
        symlink("real/sub", temporary.path().join("link")).unwrap();
        let alias = temporary.path().join("link/../source.viz.yaml");
        rejected(
            run("normalize", &source, &alias, None),
            "VIZ-PATH-0001",
            &["input", "output"],
        );
        assert_eq!(fs::read(source).unwrap(), original);
        // The lexical spelling matches the root input, but the actual output
        // is inside real/. Rejecting it would regress a valid destination.
        let different_source = input(temporary.path());
        accepted(run("normalize", &different_source, &alias, None));
        assert_eq!(fs::read(different_source).unwrap(), original);
        let mir: serde_json::Value = serde_json::from_slice(&fs::read(alias).unwrap()).unwrap();
        assert_eq!(mir["document_id"], "service-health-dashboard");
    }

    #[test]
    fn missing_dotdot_suffix_resolves_later_symlinks_before_more_dotdots() {
        let temporary = tempfile::tempdir().unwrap();
        fs::create_dir_all(temporary.path().join("real/sub")).unwrap();
        let source = input(&temporary.path().join("real"));
        let original = fs::read(&source).unwrap();
        symlink("real/sub", temporary.path().join("link")).unwrap();
        let output = temporary.path().join("missing/../link/../source.viz.yaml");
        rejected(
            run("lower", &source, &output, None),
            "VIZ-PATH-0001",
            &["input", "output"],
        );
        assert_eq!(fs::read(source).unwrap(), original);
        assert!(!temporary.path().join("missing").exists());
    }

    #[test]
    fn unresolvable_manifest_fails_before_writing_a_valid_output() {
        let temporary = tempfile::tempdir().unwrap();
        let source = input(temporary.path());
        let original = fs::read(&source).unwrap();
        let output = temporary.path().join("plot");
        fs::write(&output, SENTINEL).unwrap();
        let manifest = temporary.path().join("loop");
        symlink("loop", &manifest).unwrap();
        rejected(
            run("svg", &source, &output, Some(&manifest)),
            "VIZ-PATH-0002",
            &["manifest"],
        );
        assert_eq!(fs::read(output).unwrap(), SENTINEL);
        assert_eq!(fs::read(source).unwrap(), original);
    }

    #[test]
    fn distinct_symlink_hardlink_and_non_utf8_destinations_still_work() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let temporary = tempfile::tempdir().unwrap();
        let source = input(temporary.path());
        let original = fs::read(&source).unwrap();
        for hardlink in [false, true] {
            let target = temporary.path().join(format!("target-{hardlink}"));
            fs::write(&target, SENTINEL).unwrap();
            let output = temporary.path().join(format!("alias-{hardlink}"));
            if hardlink {
                fs::hard_link(&target, &output).unwrap();
            } else {
                symlink(&target, &output).unwrap();
            }
            accepted(run("svg", &source, &output, None));
            assert!(fs::read_to_string(target).unwrap().starts_with("<svg"));
        }
        let output = temporary
            .path()
            .join(OsString::from_vec(b"plot-\xff.svg".to_vec()));
        accepted(run("svg", &source, &output, None));
        assert!(fs::read_to_string(output).unwrap().starts_with("<svg"));
        assert_eq!(fs::read(source).unwrap(), original);
    }
    #[test]
    fn dangling_output_link_to_a_distinct_new_file_is_supported() {
        let temporary = tempfile::tempdir().unwrap();
        let source = input(temporary.path());
        let original = fs::read(&source).unwrap();
        let output = temporary.path().join("alias");
        symlink("new-target.svg", &output).unwrap();
        accepted(run("svg", &source, &output, None));
        assert!(
            fs::read_to_string(temporary.path().join("new-target.svg"))
                .unwrap()
                .starts_with("<svg")
        );
        assert_eq!(fs::read(source).unwrap(), original);
    }

    #[test]
    fn write_only_destination_does_not_need_read_permission_for_identity_check() {
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::tempdir().unwrap();
        let source = input(temporary.path());
        let output = temporary.path().join("plot");
        fs::write(&output, SENTINEL).unwrap();
        fs::set_permissions(&output, fs::Permissions::from_mode(0o200)).unwrap();
        let result = run("svg", &source, &output, None);
        fs::set_permissions(&output, fs::Permissions::from_mode(0o600)).unwrap();
        accepted(result);
        assert!(fs::read_to_string(output).unwrap().starts_with("<svg"));
    }

    #[test]
    fn inaccessible_manifest_parent_fails_closed_before_output_changes() {
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::tempdir().unwrap();
        let source = input(temporary.path());
        let original = fs::read(&source).unwrap();
        let output = temporary.path().join("plot");
        fs::write(&output, SENTINEL).unwrap();
        let blocked = temporary.path().join("blocked");
        fs::create_dir(&blocked).unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
        let manifest = blocked.join("report.json");
        let inaccessible = matches!(fs::metadata(&manifest), Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied);
        let result = inaccessible.then(|| run("svg", &source, &output, Some(&manifest)));
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
        // A privileged test runner can traverse mode-000 directories.
        if let Some(result) = result {
            rejected(result, "VIZ-PATH-0002", &["manifest"]);
            assert_eq!(fs::read(output).unwrap(), SENTINEL);
            assert_eq!(fs::read(source).unwrap(), original);
            assert!(!manifest.exists());
        }
    }
}
