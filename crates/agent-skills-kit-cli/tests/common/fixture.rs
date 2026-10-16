use std::{
    ffi::OsStr,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
pub struct Fixture(pub PathBuf);
impl Fixture {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "ask-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("owned fixture: {error}"),
            }
        }
        panic!("fixture collision budget");
    }
    pub fn source(&self, parent: &OsStr) -> PathBuf {
        let source = self.0.join(parent).join("sample");
        fs::create_dir_all(source.join("empty")).unwrap();
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: sample\ndescription: Original inert CLI fixture\n---\nData only.\n",
        )
        .unwrap();
        fs::write(
            source.join("resource.ps1"),
            b"inert resource; never executed",
        )
        .unwrap();
        source
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
