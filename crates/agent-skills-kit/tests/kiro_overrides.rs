//! Explicit Kiro CLI override policy without environment or filesystem mutation.
use agent_skills_kit::{
    catalog::{Catalog, ContextBase, Scope},
    resolver::*,
};
use std::ffi::OsString;

fn context() -> InstallContext {
    let mut context = InstallContext::new(native_platform().unwrap());
    let root = std::env::temp_dir().join("ask-kiro-inert");
    context.user_home = Some(root.join("home café"));
    context.project_root = Some(root.join("project 日本語"));
    context
}
fn rejected(context: &InstallContext) {
    assert!(matches!(
        resolve(
            &Catalog::bundled().unwrap(),
            "kiro-cli",
            Scope::User,
            context
        ),
        Err(ResolveError::InvalidOverride {
            issue: PathIssue::HostOverridePolicy,
            ..
        })
    ));
}
#[test]
fn cli_override_constraints_and_ide_boundaries() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    let home = context.user_home.clone().unwrap();
    for scope in [Scope::User, Scope::Project] {
        let cli = resolve(&catalog, "kiro-cli", scope, &context).unwrap();
        let ide = resolve(&catalog, "kiro-ide", scope, &context).unwrap();
        assert_eq!(cli.preferred_write.path, ide.preferred_write.path);
        assert_eq!(cli.discovery.len(), 1);
        assert_eq!(cli.discovery[0].location, cli.preferred_write);
    }
    for invalid in home.ancestors() {
        context.overrides.insert("KIRO_HOME".into(), invalid.into());
        rejected(&context);
        assert_eq!(
            resolve(&catalog, "kiro-ide", Scope::User, &context)
                .unwrap()
                .preferred_write
                .path,
            home.join(".kiro/skills")
        );
        assert!(resolve(&catalog, "kiro-cli", Scope::Project, &context).is_ok());
    }
    for spelling in [
        home.to_str().unwrap().to_uppercase(),
        home.to_str().unwrap().replace("é", "e\u{301}"),
    ] {
        context
            .overrides
            .insert("KIRO_HOME".into(), spelling.into());
        rejected(&context);
    }
    for valid in [
        home.join("profile"),
        home.with_file_name("home café sibling"),
    ] {
        context
            .overrides
            .insert("KIRO_HOME".into(), valid.clone().into());
        let result = resolve(&catalog, "kiro-cli", Scope::User, &context).unwrap();
        assert_eq!(result.preferred_write.path, valid.join("skills"));
        assert_eq!(
            result.preferred_write.origin,
            PathOrigin::Override("KIRO_HOME".into())
        );
    }
    context.user_home = None;
    assert!(matches!(
        resolve(&catalog, "kiro-cli", Scope::User, &context),
        Err(ResolveError::MissingContext(ContextBase::Home))
    ));
    assert!(resolve(&catalog, "kiro-cli", Scope::Project, &context).is_ok());
    context.user_home = Some(home);
    for invalid in ["", "relative", "~/profile", "bad\0root"] {
        context.overrides.insert("KIRO_HOME".into(), invalid.into());
        assert!(matches!(
            resolve(&catalog, "kiro-cli", Scope::User, &context),
            Err(ResolveError::InvalidOverride { .. })
        ));
    }
    context.overrides.insert(
        "KIRO_HOME".into(),
        context
            .user_home
            .as_ref()
            .unwrap()
            .join("../profile")
            .into(),
    );
    assert!(matches!(
        resolve(&catalog, "kiro-cli", Scope::User, &context),
        Err(ResolveError::InvalidOverride {
            issue: PathIssue::ParentComponent,
            ..
        })
    ));
}

#[cfg(unix)]
fn opaque() -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b"opaque-\xff".to_vec())
}
#[cfg(windows)]
fn opaque() -> OsString {
    use std::os::windows::ffi::OsStringExt;
    OsString::from_wide(&[0x61, 0xd800])
}
#[test]
fn explicit_override_requires_comparable_home_and_root() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = context();
    let root = context
        .user_home
        .as_ref()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    context
        .overrides
        .insert("KIRO_HOME".into(), root.join(opaque()).into());
    rejected(&context);
    context
        .overrides
        .insert("KIRO_HOME".into(), root.join("profile").into());
    context.user_home = Some(root.join(opaque()));
    rejected(&context);
    context.overrides.clear();
    assert!(resolve(&catalog, "kiro-cli", Scope::User, &context).is_ok());
}
#[cfg(windows)]
#[test]
fn other_drive_root_is_also_ignored_by_host() {
    let mut context = context();
    context
        .overrides
        .insert("KIRO_HOME".into(), std::path::PathBuf::from("Z:\\").into());
    rejected(&context);
}
