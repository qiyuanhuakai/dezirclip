#[cfg(target_os = "macos")]
fn main() -> std::io::Result<()> {
    use dzc_host::{
        Host, HostOptions,
        darwin_capture::DarwinCapture,
        darwin_clipboard::DarwinClipboardPump,
        darwin_desktop::{serve_darwin_desktop, wire_error},
        darwin_profile::DarwinProfile,
        wire::HostMode,
    };
    use dzc_platform::{
        darwin_keychain::DarwinKeyProvider, darwin_launcher::require_bundled_launcher,
        ipc::canonical_private_directory,
    };
    use security_framework::os::macos::keychain::SecKeychain;
    use std::{
        io::{self, Read, Write},
        path::PathBuf,
    };

    let mut args = std::env::args_os().skip(1);
    let directory = args
        .next()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let Some(directory) = directory.filter(|_| args.next().is_none()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "one absolute helper data directory is required",
        ));
    };
    require_bundled_launcher()?;
    let directory = canonical_private_directory(&directory)?;
    let (clipboard, actions) = DarwinClipboardPump::open()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _entered = runtime.enter();
    let keychain = SecKeychain::default().map_err(io::Error::other)?;
    let profile = DarwinProfile::open(&directory.join("profile"), |id| {
        DarwinKeyProvider::for_profile(keychain, id)
    })?;
    let host = Host::new(HostOptions {
        mode: HostMode::Service,
        history: profile.history.clone(),
        directory: profile.directory.clone(),
        actions,
    })
    .map_err(wire_error)?;
    let grant = host.grants().issue_bundled_desktop().map_err(wire_error)?;
    let capture = match DarwinCapture::start(&host, &clipboard) {
        Ok(capture) => Some(capture),
        Err(dzc_host::wire::WireErrorCode::Busy) => {
            host.set_capture_diagnostic("capture_ownership_unavailable")
                .map_err(wire_error)?;
            None
        }
        Err(error) => return Err(wire_error(error)),
    };

    {
        let mut output = io::stdout().lock();
        serde_json::to_writer(
            &mut output,
            &serde_json::json!({
                "instance": host.instance_id(), "grant": grant.token(), "socket": profile.endpoint.address(),
            }),
        )?;
        writeln!(output)?;
        output.flush()?;
    }
    let (closed, parent) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("dzc-darwin-parent".into())
        .spawn(move || {
            let result = match io::stdin().lock().read(&mut [0]) {
                Ok(0) => Ok(()),
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected launcher command",
                )),
                Err(error) => Err(error),
            };
            let _ = closed.send(result);
        })?;
    runtime.block_on(serve_darwin_desktop(
        host,
        profile.listener,
        clipboard,
        capture,
        grant,
        async { parent.await.map_err(io::Error::other)? },
    ))
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "requires macOS",
    ))
}
