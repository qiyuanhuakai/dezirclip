use dzc_core::{
    api::{ClipboardEntry, PrivacyAccessIntent, SensitiveAccess},
    crypto::{KeyProvider, KEYCHAIN_ENCRYPT_PREFIX},
};
use dzc_host::darwin_profile::DarwinProfile;
use dzc_platform::{darwin_keychain::DarwinKeyProvider, ipc::canonical_private_directory};
use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};
use serde_json::json;
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Write},
    os::unix::fs::{symlink, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// Manufactured input, kept out of argv, discovery and the program's logs.
const MARKER: &str = "dzc-darwin-cross-process-protected-history";

pub fn child() -> Result<()> {
    let mut args = std::env::args_os().skip(2);
    let operation = args.next().ok_or("profile operation required")?;
    let directory = PathBuf::from(args.next().ok_or("profile directory required")?);
    let keychain_path = PathBuf::from(args.next().ok_or("keychain path required")?);
    let mut secret = String::new();
    let mut input = io::stdin().lock();
    input.read_line(&mut secret)?;
    let mut keychain = SecKeychain::open(&keychain_path)?;
    keychain.unlock(Some(secret.trim_end()))?;
    drop(secret);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _entered = runtime.enter();
    let profile = DarwinProfile::open(&directory, |id| {
        DarwinKeyProvider::for_profile(keychain, id)
    })
    .map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("profile bootstrap: {:?}: {error}", error.kind()),
        )
    })?;
    match operation.to_str() {
        Some("write") => {
            let entry: ClipboardEntry = serde_json::from_value(json!({
                "id": 0, "content_type": "text", "content": MARKER, "preview": MARKER,
                "source_app": "persistent fixture", "timestamp": 1, "is_pinned": false, "tags": ["sensitive"]
            }))?;
            profile.history.save(&entry, None)?;
        }
        Some("read" | "hold") => {
            let privacy = profile.history.privacy();
            let denied = privacy.authorize_read(
                PrivacyAccessIntent::InteractiveRead,
                SensitiveAccess::Denied,
                None,
                2,
            )?;
            let explicit = privacy.authorize_read(
                PrivacyAccessIntent::InteractiveRead,
                SensitiveAccess::ExplicitUserAction,
                None,
                2,
            )?;
            if profile
                .history
                .get_entry_authorized(1, &denied, 2)?
                .is_some()
                || profile
                    .history
                    .get_entry_authorized(1, &explicit, 2)?
                    .as_ref()
                    .map(|item| item.content.as_str())
                    != Some(MARKER)
            {
                return Err("reopened history did not enforce protected read policy".into());
            }
        }
        _ => return Err("invalid profile operation".into()),
    }
    println!("{}", profile.id);
    io::stdout().flush()?;
    if operation == "hold" {
        let mut end = String::new();
        input.read_line(&mut end)?;
    }
    Ok(())
}

fn launch(operation: &str, directory: &Path, keychain: &Path, secret: &str) -> Result<Child> {
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--profile-child", operation])
        .arg(directory)
        .arg(keychain)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    writeln!(
        child.stdin.as_mut().ok_or("profile child input missing")?,
        "{secret}"
    )?;
    Ok(child)
}

fn profile_id(output: Output) -> Result<Uuid> {
    if !output.status.success() {
        return Err(format!(
            "profile child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(Uuid::parse_str(
        std::str::from_utf8(&output.stdout)?.trim(),
    )?)
}

fn expect_denial(output: Output, expected: &str) -> Result<()> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success()
        || !output.stdout.is_empty()
        || !stderr.contains("profile bootstrap:")
        || !stderr.contains(expected)
    {
        return Err(format!(
            "expected failed bootstrap ({expected}), received {} {stderr}",
            output.status
        )
        .into());
    }
    Ok(())
}

fn new_keychain(path: &Path, password: &str) -> Result<SecKeychain> {
    Ok(CreateOptions::new()
        .password(password)
        .prompt_user(false)
        .create(path)?)
}

pub fn verify() -> Result<()> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(2)
            .ok_or("private verification directory required")?,
    );
    let root = canonical_private_directory(&directory)?;
    let secret = Uuid::new_v4().simple().to_string();
    let keychain = root.join("persistent.keychain-db");
    drop(new_keychain(&keychain, &secret)?);
    let profile = root.join("profile");
    let id = profile_id(launch("write", &profile, &keychain, &secret)?.wait_with_output()?)?;
    let reopened = profile_id(launch("read", &profile, &keychain, &secret)?.wait_with_output()?)?;
    if id != reopened {
        return Err("profile identity changed across helper launches".into());
    }
    let database = fs::read(profile.join("history.sqlite3"))?;
    let contains = |needle: &str| {
        database
            .windows(needle.len())
            .any(|bytes| bytes == needle.as_bytes())
    };
    if contains(MARKER) || !contains(KEYCHAIN_ENCRYPT_PREFIX) {
        return Err("SQLite did not persist protected history in the Keychain envelope".into());
    }

    // The canonical parent path and its alias select the same live owner.
    let alias = root.join("alias");
    symlink(&root, &alias)?;
    let mut held = launch("hold", &profile, &keychain, &secret)?;
    let mut ready = String::new();
    BufReader::new(held.stdout.take().ok_or("profile child output missing")?)
        .read_line(&mut ready)?;
    let concurrent = launch("read", &alias.join("profile"), &keychain, &secret)?.wait_with_output();
    drop(held.stdin.take());
    let held_output = held.wait_with_output()?;
    if !held_output.status.success() || ready.trim() != id.to_string() {
        return Err(format!(
            "profile owner failed: {}",
            String::from_utf8_lossy(&held_output.stderr)
        )
        .into());
    }
    expect_denial(concurrent?, "AddrInUse")?;

    let empty_keychain = root.join("empty.keychain-db");
    let empty = new_keychain(&empty_keychain, &secret)?;
    expect_denial(
        launch("read", &profile, &empty_keychain, &secret)?.wait_with_output()?,
        "KeyUnavailable",
    )?;
    let empty_provider = DarwinKeyProvider::for_profile(empty, id)?;
    if empty_provider.verify_existing().is_ok() {
        return Err("existing profile provisioned a replacement key".into());
    }
    empty_provider.provision_new_profile()?;
    expect_denial(
        launch("read", &profile, &empty_keychain, &secret)?.wait_with_output()?,
        "KeyUnavailable",
    )?;

    let identity = profile.join("profile-id");
    let saved_identity = root.join("saved-profile-id");
    fs::rename(&identity, &saved_identity)?;
    expect_denial(
        launch("read", &profile, &keychain, &secret)?.wait_with_output()?,
        "invalid or incomplete",
    )?;
    if identity.exists() {
        return Err("identity was silently replaced".into());
    }
    symlink(&saved_identity, &identity)?;
    expect_denial(
        launch("read", &profile, &keychain, &secret)?.wait_with_output()?,
        "profile bootstrap:",
    )?;
    fs::remove_file(&identity)?;
    fs::rename(&saved_identity, &identity)?;
    if profile_id(launch("read", &profile, &keychain, &secret)?.wait_with_output()?)? != id {
        return Err("failed bootstrap modified the original profile".into());
    }

    // Simulate interruption after inserting the key and before creating SQLite.
    let interrupted = canonical_private_directory(&root.join("interrupted"))?;
    let interrupted_id = Uuid::new_v4();
    let mut identity = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(interrupted.join("profile-id"))?;
    write!(identity, "v1 creating {interrupted_id}")?;
    identity.sync_all()?;
    drop(identity);
    let mut opened = SecKeychain::open(&keychain)?;
    opened.unlock(Some(&secret))?;
    let provider = DarwinKeyProvider::for_profile(opened, interrupted_id)?;
    provider.provision_new_profile()?;
    let before = provider.seal_text(MARKER)?;
    let recovered =
        profile_id(launch("write", &interrupted, &keychain, &secret)?.wait_with_output()?)?;
    if recovered != interrupted_id
        || provider.open_text(&before).as_deref() != Some(MARKER)
        || profile_id(launch("read", &interrupted, &keychain, &secret)?.wait_with_output()?)?
            != interrupted_id
    {
        return Err("interrupted profile bootstrap changed the profile identity or its key".into());
    }
    let mismatch = canonical_private_directory(&root.join("mismatch"))?;
    fs::copy(profile.join("profile-id"), mismatch.join("profile-id"))?;
    fs::copy(
        interrupted.join("history.sqlite3"),
        mismatch.join("history.sqlite3"),
    )?;
    expect_denial(
        launch("read", &mismatch, &keychain, &secret)?.wait_with_output()?,
        "KeyUnavailable",
    )?;
    println!("Verified the signed Darwin profile across processes: stable ID, protected SQLite read, exclusive alias, missing key and identity denial, mismatched database rejection, interrupted provisioning");
    Ok(())
}
