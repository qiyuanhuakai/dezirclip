//! Preconditions for tests that change the clipboard or register shortcuts.
use std::{env, path::PathBuf};

pub(crate) fn private_runtime(flag: &str) -> PathBuf {
    assert_eq!(env::var(flag).as_deref(), Ok("1"), "set {flag} only inside the disposable native fixture");
    let root = PathBuf::from(env::var_os("XDG_RUNTIME_DIR").expect("private runtime directory"));
    assert!(root.is_absolute());
    assert_eq!(
        std::fs::read(root.join("dezirclip-isolated-wayland-fixture")).unwrap(),
        b"private synthetic clipboard only\n"
    );
    root
}

pub(crate) fn private_x11(flag: &str) -> PathBuf {
    let root = private_runtime(flag);
    assert!(!crate::wayland::requested(), "expected the private X11 session");
    assert!(
        env::var("DISPLAY").is_ok_and(|display| {
            display.strip_prefix(':').is_some_and(|number| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
        }),
        "private Xvfb DISPLAY is required"
    );
    root
}
