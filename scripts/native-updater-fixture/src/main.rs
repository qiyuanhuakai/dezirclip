use std::path::PathBuf;
use tauri_plugin_updater::UpdaterExt;

fn validate_isolation(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("DEZIRCLIP_ACCEPTANCE_INSIDE").as_deref() != Ok("1")
        || PathBuf::from(std::env::var("DEZIRCLIP_ACCEPTANCE_ROOT")?).canonicalize()? != root
    {
        return Err("run this helper through test-native-appimage.py".into());
    }
    let host: serde_json::Value =
        serde_json::from_str(&std::env::var("DEZIRCLIP_ACCEPTANCE_HOST_NAMESPACES")?)?;
    for name in ["user", "pid", "net"] {
        let actual = std::fs::read_link(format!("/proc/self/ns/{name}"))?;
        let expected = host[name].as_str().ok_or("host namespace proof missing")?;
        if actual.to_str() == Some(expected) {
            return Err(format!("private {name} namespace required").into());
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let directory =
        PathBuf::from(arguments.next().ok_or("fixture directory required")?).canonicalize()?;
    let read_only = match arguments.next().as_deref() {
        None => false,
        Some("--read-only") => true,
        Some(_) => return Err("unknown fixture mode".into()),
    };
    if arguments.next().is_some() {
        return Err("unexpected extra fixture argument".into());
    }
    let root = directory.parent().ok_or("fixture root missing")?;
    let in_fixture = root.parent() == Some(std::env::temp_dir().canonicalize()?.as_path())
        && root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("dezirclip-native-acceptance-"))
        && !root.join(".native-updater-fixture").is_symlink()
        && std::fs::read_to_string(root.join(".native-updater-fixture"))?
            == "Synthetic clipboard acceptance; no production key or user profile.\n"
        && directory.file_name().is_some_and(|name| name == "scenario");
    if !in_fixture {
        return Err("only the disposable AppImage fixture is allowed".into());
    }
    validate_isolation(root)?;
    let target = directory.join("current.AppImage");
    let candidate_path = directory.join("DezirClip_0.2.0-alpha.2_amd64.AppImage");
    for input in [
        target.clone(),
        candidate_path.clone(),
        candidate_path.with_extension("AppImage.sig"),
        directory.join("test-updater.key.pub"),
    ] {
        if !std::fs::symlink_metadata(&input)?.is_file()
            || input.canonicalize()?.parent() != Some(directory.as_path())
        {
            return Err("fixture inputs must be regular files directly inside scenario".into());
        }
    }
    let old = std::fs::read(&target)?;
    let candidate = std::fs::read(&candidate_path)?;
    let signature = std::fs::read_to_string(candidate_path.with_extension("AppImage.sig"))?;
    let public_key = std::fs::read_to_string(directory.join("test-updater.key.pub"))?;
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({"pubkey": public_key.trim(), "requireSignedVersion": true}),
    );
    let app = tauri::test::mock_builder()
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(public_key.trim())
                .build(),
        )
        .build(context)?;
    let updater = app
        .updater_builder()
        .target("linux-x86_64")
        .executable_path(&target)
        .endpoints(vec!["https://example.invalid/never-requested".parse()?])?
        .configure_client(|_| panic!("offline fixture must not make network requests"))
        .build()?;
    let manifest = serde_json::json!({
        "version": "0.2.0-alpha.2",
        "platforms": { "linux-x86_64": {
            "url": "https://example.invalid/DezirClip_0.2.0-alpha.2_amd64.AppImage",
            "signature": signature.trim(),
        }},
    });
    let update = updater
        .restore_update(manifest.clone())?
        .ok_or("update missing")?;
    update.verify(&candidate)?;
    if read_only {
        assert!(
            update.install(&candidate).is_err(),
            "read-only installation succeeded"
        );
        assert_eq!(
            std::fs::read(&target)?,
            old,
            "read-only installation changed bytes"
        );
        println!(
            "{}",
            serde_json::json!({"official_plugin": "2.14.0", "read_only_rejected": true,
            "installed_bytes_unchanged": true, "signature_valid": true, "network_requests": 0})
        );
        return Ok(());
    }
    let mut altered = candidate.clone();
    let last = altered.len().checked_sub(1).ok_or("candidate empty")?;
    altered[last] ^= 1;
    assert!(
        update.verify(&altered).is_err(),
        "tampered signature verified"
    );
    assert!(update.install(&altered).is_err(), "tampered bytes accepted");
    assert_eq!(
        std::fs::read(&target)?,
        old,
        "tampered bytes changed installed file"
    );
    let mut wrong_version = manifest.clone();
    wrong_version["version"] = "0.2.0-alpha.3".into();
    let mismatched = updater
        .restore_update(wrong_version)?
        .ok_or("mismatched update missing")?;
    assert!(
        mismatched.verify(&candidate).is_err(),
        "wrong signed version verified"
    );
    assert!(
        mismatched.install(&candidate).is_err(),
        "unsigned manifest version accepted"
    );
    assert_eq!(
        std::fs::read(&target)?,
        old,
        "wrong version changed installed file"
    );
    update.install(&candidate)?;
    assert_eq!(
        std::fs::read(&target)?,
        candidate,
        "official installer did not replace bytes"
    );
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&target)?.permissions().mode() & 0o777,
            0o755
        );
    }
    println!(
        "{}",
        serde_json::json!({"official_plugin": "2.14.0", "real_appimage_replaced": true,
        "tampered_payload_unchanged": true, "manifest_version_tamper_unchanged": true,
        "executable_permissions_preserved": true, "namespace_isolation_verified": true, "network_requests": 0})
    );
    Ok(())
}
