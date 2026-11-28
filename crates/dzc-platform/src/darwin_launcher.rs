//! Verifies the running app that owns a bundled helper's private launch pipes.
use core_foundation::url::CFURL;
use security_framework::os::macos::code_signing::{
    Flags, GuestAttributes, SecCode, SecRequirement, SecStaticCode,
};
use std::{
    ffi::{CStr, OsStr},
    fs, io,
    mem::MaybeUninit,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

pub fn require_bundled_launcher() -> io::Result<()> {
    for descriptor in [libc::STDIN_FILENO, libc::STDOUT_FILENO] {
        let mut metadata = MaybeUninit::<libc::stat>::uninit();
        // The inherited descriptors carry the only bootstrap and lifetime channels.
        if unsafe { libc::fstat(descriptor, metadata.as_mut_ptr()) } != 0
            || unsafe { metadata.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFIFO
        {
            return Err(denied("bootstrap descriptors must be pipes"));
        }
    }

    let helper = fs::canonicalize(std::env::current_exe()?)?;
    let Some(contents) = helper.parent().and_then(Path::parent) else {
        return Err(denied("helper must be inside Contents/Helpers"));
    };
    let Some(bundle) = contents.parent() else {
        return Err(denied("helper must be inside Contents/Helpers"));
    };
    if helper != contents.join("Helpers/dzc-darwin-host")
        || contents.file_name() != Some(OsStr::new("Contents"))
        || bundle.extension() != Some(OsStr::new("app"))
    {
        return Err(denied("helper must be inside Contents/Helpers"));
    }

    let pid = unsafe { libc::getppid() };
    if pid <= 1 {
        return Err(denied("launcher exited"));
    }
    let path = parent_executable(pid)?;
    if path.parent() != Some(contents.join("MacOS").as_path()) {
        return Err(denied("parent is outside Contents/MacOS"));
    }

    let requirement: SecRequirement = r#"info[CFBundlePackageType] = "APPL""#
        .parse()
        .map_err(|error| signature_error("app requirement", error))?;
    let mut attributes = GuestAttributes::new();
    attributes.set_pid(pid);
    let parent = SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE)
        .map_err(|error| signature_error("running app lookup", error))?;
    parent
        .check_validity(Flags::NONE, &requirement)
        .map_err(|error| signature_error("running app", error))?;
    let url = CFURL::from_path(bundle, true).ok_or_else(|| denied("invalid bundle URL"))?;
    let signed = SecStaticCode::from_path(&url, Flags::NONE)
        .map_err(|error| signature_error("app bundle lookup", error))?;
    signed
        .check_validity(
            Flags::NO_NETWORK_ACCESS | Flags::CHECK_NESTED_CODE | Flags::STRICT_VALIDATE,
            &requirement,
        )
        .map_err(|error| signature_error("app bundle", error))?;
    if unsafe { libc::getppid() } != pid {
        return Err(denied("launcher changed during verification"));
    }
    Ok(())
}

fn parent_executable(pid: libc::pid_t) -> io::Result<PathBuf> {
    let mut buffer = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let count = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if count <= 0 || count as usize >= buffer.len() {
        return Err(denied("parent executable is unavailable"));
    }
    let path = CStr::from_bytes_until_nul(&buffer)
        .map_err(|_| denied("parent executable is unavailable"))?;
    fs::canonicalize(Path::new(OsStr::from_bytes(path.to_bytes())))
        .map_err(|_| denied("parent executable is unavailable"))
}

fn denied(reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("helper requires private pipes from its signed app launcher: {reason}"),
    )
}

fn signature_error(subject: &str, error: security_framework::base::Error) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{subject} signature validation failed ({})", error.code()),
    )
}
