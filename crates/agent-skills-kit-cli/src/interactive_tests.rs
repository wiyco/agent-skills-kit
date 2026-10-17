use super::*;
use crate::test_fixture as fixture;
use agent_skills_kit::{
    catalog::Scope,
    resolver::{InstallContext, native_platform, resolve},
};
use fixture::Fixture;
use std::{ffi::OsStr, fs, io::Cursor, path::Path};
fn root(fixture: &Fixture) -> PathBuf {
    let root = fixture.0.join("explicit root café");
    fs::create_dir(&root).unwrap();
    root
}
fn input(source: &Path, targets: &str, scope: &str, root: &Path, answer: &str) -> String {
    format!(
        "{}\n{targets}\n{scope}\n{}\n{answer}\n",
        source.display(),
        root.display()
    )
}
#[test]
fn both_scopes_install_selected_core_targets_only_after_the_displayed_plan() {
    for (scope, label) in [(Scope::User, "user"), (Scope::Project, "project")] {
        let fixture = Fixture::new();
        let source = fixture.source(OsStr::new("source"));
        let root = root(&fixture);
        let mut prompts = vec![];
        let answers = input(
            &source,
            "codex,codex-cli,claude-code,opencode",
            label,
            &root,
            "yes",
        );
        let answers = if scope == Scope::User {
            answers.replace('\n', "\r\n")
        } else {
            answers
        };
        let output = execute(&mut Cursor::new(answers), &mut prompts).unwrap();
        assert_eq!(output.exit_code, 0, "{output:?}");
        assert_eq!(
            output.data["report"]["outcomes"].as_array().unwrap().len(),
            3
        );
        let text = String::from_utf8(prompts).unwrap();
        assert!(text.find("create ").unwrap() < text.find("Apply this exact plan?").unwrap());
        assert!(text.contains("compatible hosts may discover"));
        let mut context = InstallContext::new(native_platform().unwrap());
        if scope == Scope::User {
            context.user_home = Some(root.clone());
        } else {
            context.project_root = Some(root.clone());
        }
        for target in ["codex", "claude-code", "opencode"] {
            let path = resolve(&Catalog::bundled().unwrap(), target, scope, &context)
                .unwrap()
                .preferred_write
                .path
                .join("sample");
            assert_eq!(
                fs::read(path.join("SKILL.md")).unwrap(),
                fs::read(source.join("SKILL.md")).unwrap()
            );
            assert_eq!(
                fs::read(path.join("resource.ps1")).unwrap(),
                b"inert resource; never executed"
            );
        }
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
    }
}
#[test]
fn every_prompt_cancels_on_q_blank_or_eof_without_destination_writes() {
    for stop in 0..5 {
        for cancel in [Some("q"), Some(""), None] {
            let fixture = Fixture::new();
            let source = fixture.source(OsStr::new("source"));
            let root = root(&fixture);
            let answers = [
                source.to_str().unwrap(),
                "codex,claude-code,opencode",
                "project",
                root.to_str().unwrap(),
            ];
            let mut text = answers[..stop.min(4)]
                .iter()
                .map(|value| format!("{value}\n"))
                .collect::<String>();
            if let Some(cancel) = cancel {
                text.push_str(cancel);
                text.push('\n');
            }
            let mut prompts = vec![];
            let output = execute(&mut Cursor::new(text), &mut prompts).unwrap();
            assert_eq!(output.exit_code, 3, "stop={stop}: {output:?}");
            let last_prompt = [
                "Skill source path: ",
                "Target IDs/aliases (comma separated): ",
                "Scope (user or project): ",
                "Explicit project root: ",
                "Apply this exact plan? Type yes to approve: ",
            ][stop];
            assert!(
                String::from_utf8(prompts).unwrap().ends_with(last_prompt),
                "cancellation must stop at the current prompt"
            );
            assert_eq!(output.data["apply_attempted"], false);
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
            assert_eq!(
                fs::read(source.join("resource.ps1")).unwrap(),
                b"inert resource; never executed"
            );
        }
    }
}
#[test]
fn cancelled_confirmation_retains_the_same_plan_as_non_interactive_library_selection() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let root = root(&fixture);
    let options = PlanArgs {
        source: source.clone(),
        targets: vec!["codex".into(), "opencode".into()],
        scope: Scope::Project,
        user_home: None,
        project_root: Some(root.clone()),
    };
    let expected = planning::execute(options).unwrap().data;
    let output = execute(
        &mut Cursor::new(input(&source, "codex,opencode", "project", &root, "no")),
        &mut vec![],
    )
    .unwrap();
    assert_eq!(output.exit_code, 3);
    assert_eq!(output.data["plan"], expected);
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}
#[test]
fn invalid_scope_unknown_target_and_oversized_input_never_default_to_a_selection() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let root = root(&fixture);
    for text in [
        input(&source, "unknown", "project", &root, "yes"),
        input(&source, "codex", "workspace", &root, "yes"),
        "x".repeat(MAX_INPUT + 1),
    ] {
        let error = execute(&mut Cursor::new(text), &mut vec![]).unwrap_err();
        assert_eq!(error.code, 2);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }
    let error = execute(&mut Cursor::new(vec![0xff, b'\n']), &mut vec![]).unwrap_err();
    assert_eq!(error.code, 2);
}
#[test]
fn conflicts_and_host_reservations_are_displayed_without_a_confirmation_prompt() {
    for reservation in [false, true] {
        let fixture = Fixture::new();
        let original = fixture.source(OsStr::new("source"));
        let root = root(&fixture);
        let source = if reservation {
            let path = original.parent().unwrap().join("synced");
            fs::rename(&original, &path).unwrap();
            fs::write(path.join("SKILL.md"),b"---\nname: synced\ndescription: Original reserved-name fixture\n---\nData only.\n").unwrap();
            path
        } else {
            original
        };
        let target = if reservation { "claude-code" } else { "codex" };
        let mut edited = None;
        if !reservation {
            let options = PlanArgs {
                source: source.clone(),
                targets: vec![target.into()],
                scope: Scope::Project,
                user_home: None,
                project_root: Some(root.clone()),
            };
            let output = installation::execute(options, true, false).unwrap();
            assert_eq!(output.exit_code, 0);
            let mut context = InstallContext::new(native_platform().unwrap());
            context.project_root = Some(root.clone());
            let path = resolve(
                &Catalog::bundled().unwrap(),
                target,
                Scope::Project,
                &context,
            )
            .unwrap()
            .preferred_write
            .path
            .join("sample/resource.ps1");
            fs::write(&path, b"manual edit").unwrap();
            edited = Some(path);
        }
        let mut prompts = vec![];
        let output = execute(
            &mut Cursor::new(input(&source, target, "project", &root, "yes")),
            &mut prompts,
        )
        .unwrap();
        assert_eq!(output.exit_code, 2);
        assert_eq!(output.data["apply_attempted"], false);
        assert_eq!(
            output.data["plan"]["destinations"][0]["action"],
            if reservation {
                "unsupported"
            } else {
                "conflict"
            }
        );
        let prompts = String::from_utf8(prompts).unwrap();
        assert!(prompts.contains("This plan is blocked"));
        assert!(!prompts.contains("Apply this exact plan?"));
        if let Some(path) = edited {
            assert_eq!(fs::read(path).unwrap(), b"manual edit");
        } else {
            assert_eq!(fs::read_dir(root).unwrap().count(), 0);
        }
    }
}
#[test]
fn source_change_at_confirmation_is_rejected_using_the_original_retained_plan() {
    struct MutateAtConfirmation {
        source: PathBuf,
        changed: bool,
        bytes: Vec<u8>,
    }
    impl Write for MutateAtConfirmation {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if !self.changed && String::from_utf8_lossy(bytes).contains("Apply this exact plan?") {
                fs::write(
                    self.source.join("resource.ps1"),
                    b"source changed after preview",
                )?;
                self.changed = true;
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let root = root(&fixture);
    let mut prompts = MutateAtConfirmation {
        source: source.clone(),
        changed: false,
        bytes: vec![],
    };
    let output = execute(
        &mut Cursor::new(input(&source, "codex", "project", &root, "yes")),
        &mut prompts,
    )
    .unwrap();
    assert!(prompts.changed);
    assert_eq!(output.exit_code, 4);
    assert!(!output.data["report"]["preflight_error"].is_null());
    assert_eq!(
        output.data["report"]["outcomes"][0]["status"],
        "not_attempted"
    );
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn existing_host_directories_never_preselect_a_target() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let root = root(&fixture);
    let mut context = InstallContext::new(native_platform().unwrap());
    context.project_root = Some(root.clone());
    let container = resolve(
        &Catalog::bundled().unwrap(),
        "codex",
        Scope::Project,
        &context,
    )
    .unwrap()
    .preferred_write
    .path;
    fs::create_dir_all(&container).unwrap();
    let mut prompts = vec![];
    let output = execute(
        &mut Cursor::new(format!("{}\n\n", source.display())),
        &mut prompts,
    )
    .unwrap();
    assert_eq!(output.exit_code, 3);
    assert!(output.data["plan"].is_null());
    assert!(
        !String::from_utf8(prompts)
            .unwrap()
            .contains("Scope (user or project):")
    );
    assert_eq!(fs::read_dir(container).unwrap().count(), 0);
}

#[test]
fn partial_yes_at_eof_is_cancellation_not_approval() {
    let fixture = Fixture::new();
    let source = fixture.source(OsStr::new("source"));
    let root = root(&fixture);
    let text = format!(
        "{}\ncodex\nproject\n{}\nyes",
        source.display(),
        root.display()
    );
    let output = execute(&mut Cursor::new(text), &mut vec![]).unwrap();
    assert_eq!(
        output.exit_code, 3,
        "EOF must not approve an unterminated line"
    );
    assert_eq!(output.data["apply_attempted"], false);
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}
