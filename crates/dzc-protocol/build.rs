use std::{env, path::{Path, PathBuf}, process::Command};

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").env("GIT_OPTIONAL_LOCKS", "0").arg("-C").arg(root).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn valid_revision(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn copied_build_input(name: &str) -> bool {
    name == "Cargo.toml" || name == "Cargo.lock"
        || ((name.starts_with("crates/") || name.starts_with("src-tauri/"))
            && (name.ends_with(".rs") || name.ends_with("/Cargo.toml") || name.ends_with("/Cargo.lock")))
        || name.starts_with("src-tauri/src/cli/shell/")
        || name == "skills/dezirclip-theme/assets/forest.theme.json"
}

fn main() {
    for name in ["DZC_BUILD_REVISION", "DZC_BUILD_SOURCE_ROOT", "GITHUB_ACTIONS", "GITHUB_SHA"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let root = root.canonicalize().unwrap();
    let source_root = env::var_os("DZC_BUILD_SOURCE_ROOT").map(PathBuf::from);
    if source_root.is_some() {
        assert_eq!(env::var("GITHUB_ACTIONS").as_deref(), Ok("true"), "External source provenance is restricted to Actions builds");
        assert!(env::var_os("DZC_BUILD_REVISION").is_some(), "External source provenance requires an explicit revision");
    }
    let git_root = source_root.as_ref().map(|path| path.canonicalize().expect("Source checkout must exist")).unwrap_or_else(|| root.clone());
    let owns_checkout = git(&git_root, &["rev-parse", "--show-toplevel"])
        .and_then(|value| PathBuf::from(value).canonicalize().ok()).as_ref() == Some(&git_root);
    let head = owns_checkout.then(|| git(&git_root, &["rev-parse", "HEAD"]))
        .flatten().filter(|value| valid_revision(value)).map(|value| value.to_ascii_lowercase());
    let explicit = env::var("DZC_BUILD_REVISION").ok();
    if let Some(value) = &explicit {
        assert!(valid_revision(value), "DZC_BUILD_REVISION must be a full 40-character source revision");
        assert_eq!(head.as_deref(), Some(value.to_ascii_lowercase()).as_deref(), "DZC_BUILD_REVISION must match this verified Git checkout");
        if env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
            assert_eq!(env::var("GITHUB_SHA").ok().as_deref().map(str::to_ascii_lowercase), Some(value.to_ascii_lowercase()), "Actions build revision must match GITHUB_SHA");
        }
    }
    let dirty = owns_checkout.then(|| git(&git_root, &["status", "--porcelain", "--untracked-files=no"]))
        .flatten().map(|value| !value.is_empty());
    if owns_checkout {
        let mut watched_refs = vec!["HEAD".to_owned(), "index".to_owned(), "packed-refs".to_owned()];
        if let Some(reference) = git(&git_root, &["symbolic-ref", "-q", "HEAD"]) { watched_refs.push(reference); }
        for name in watched_refs {
            if let Some(path) = git(&git_root, &["rev-parse", "--git-path", &name]) {
                let path = PathBuf::from(path);
                println!("cargo:rerun-if-changed={}", if path.is_absolute() { path } else { git_root.join(path) }.display());
            }
        }
        let output = Command::new("git").env("GIT_OPTIONAL_LOCKS", "0")
            .arg("-C").arg(&git_root).args(["ls-files", "-z"]).output()
            .expect("Cannot launch tracked build provenance enumeration");
        assert!(output.status.success(), "Cannot enumerate tracked build provenance");
        for name in String::from_utf8_lossy(&output.stdout).split('\0').filter(|name| !name.is_empty()) {
            println!("cargo:rerun-if-changed={}", git_root.join(name).display());
            if git_root != root && copied_build_input(name) {
                let actual = root.join(name);
                assert_eq!(std::fs::read(&actual).expect("Copied Rust/Cargo input is missing"), std::fs::read(git_root.join(name)).expect("Original Rust/Cargo input is unreadable"), "Copied Rust/Cargo input differs from the declared source checkout: {name}");
                println!("cargo:rerun-if-changed={}", actual.display());
            }
        }
    }
    println!("cargo:rustc-env=DZC_SOURCE_REVISION={}", head.as_deref().unwrap_or("unknown"));
    println!("cargo:rustc-env=DZC_SOURCE_STATE={}", match (&head, dirty) {
        (Some(_), Some(false)) => "verified_checkout", (Some(_), Some(true)) => "modified_checkout", _ => "unknown",
    });
    println!("cargo:rustc-env=DZC_REVISION_ORIGIN={}", if explicit.is_some() { "explicit_checkout" } else if head.is_some() { "local_git" } else { "unknown" });
    println!("cargo:rustc-env=DZC_BUILD_TARGET={}", env::var("TARGET").unwrap());
}
