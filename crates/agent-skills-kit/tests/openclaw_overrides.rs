//! Native encoding and namespace boundaries for the default-state precondition.
use agent_skills_kit::{
    catalog::{Catalog, Scope},
    resolver::*,
};
use std::ffi::OsString;
#[cfg(unix)]
fn opaque() -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b"home-\xff".to_vec())
}
#[cfg(windows)]
fn opaque() -> OsString {
    use std::os::windows::ffi::OsStringExt;
    OsString::from_wide(&[0x61, 0xd800])
}
#[test]
fn default_state_comparison_preserves_native_opaque_units() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = InstallContext::new(native_platform().unwrap());
    let home = std::env::temp_dir()
        .join("ask-openclaw-inert")
        .join(opaque());
    context.user_home = Some(home.clone());
    context
        .overrides
        .insert("OPENCLAW_HOME".into(), home.clone().into());
    context
        .overrides
        .insert("OPENCLAW_STATE_DIR".into(), home.join(".openclaw").into());
    assert_eq!(
        resolve(&catalog, "openclaw-local", Scope::User, &context)
            .unwrap()
            .preferred_write
            .path,
        home.join(".agents/skills")
    );
    context
        .overrides
        .insert("OPENCLAW_PROFILE".into(), "team".into());
    assert!(matches!(
        resolve(&catalog, "openclaw-local", Scope::User, &context),
        Err(ResolveError::InvalidOverride {
            issue: PathIssue::HostOverridePolicy,
            ..
        })
    ));
    assert!(resolve(&catalog, "codex", Scope::User, &context).is_ok());
}
#[cfg(windows)]
#[test]
fn equivalent_verbatim_state_spelling_is_conservatively_refused() {
    let catalog = Catalog::bundled().unwrap();
    let mut context = InstallContext::new(native_platform().unwrap());
    let home = std::env::temp_dir().join("ask-openclaw-inert");
    context.user_home = Some(home.clone());
    context.overrides.insert(
        "OPENCLAW_STATE_DIR".into(),
        format!(r"\\?\{}", home.join(".openclaw").display()).into(),
    );
    assert!(matches!(
        resolve(&catalog, "openclaw-local", Scope::User, &context),
        Err(ResolveError::InvalidOverride {
            issue: PathIssue::HostOverridePolicy,
            ..
        })
    ));
}
