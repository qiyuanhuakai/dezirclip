use dbus::arg::{OwnedFd, PropMap, Variant};
use dbus::blocking::Connection;
use dbus::message::MatchRule;
use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const PORTAL_BUS: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";
const HANDLE_TOKEN: &str = "dezirclip_share";

fn open_attachment(path: &str) -> io::Result<OwnedFd> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("ShareAttachmentUnavailable"));
    }
    Ok(file.into())
}

/// Open a draft in the desktop's email client, with no recipient set.
pub fn compose_share_email(
    body: Option<String>,
    attachments: Vec<String>,
    authorized: impl FnOnce() -> bool,
) -> io::Result<bool> {
    let connection = Connection::new_session().map_err(io::Error::other)?;
    compose_on(&connection, body, attachments, authorized)
}

fn compose_on(
    connection: &Connection,
    body: Option<String>,
    attachments: Vec<String>,
    authorized: impl FnOnce() -> bool,
) -> io::Result<bool> {
    let sender = connection
        .unique_name()
        .trim_start_matches(':')
        .replace('.', "_");
    let expected_path = dbus::Path::new(format!("{PORTAL_PATH}/request/{sender}/{HANDLE_TOKEN}"))
        .map_err(io::Error::other)?;
    let (send, receive) = mpsc::sync_channel(1);
    let rule = MatchRule::new_signal(REQUEST_INTERFACE, "Response")
        .with_sender(PORTAL_BUS)
        .with_path(expected_path.clone());
    connection
        .add_match(rule, move |(code, _): (u32, PropMap), _, _| {
            let _ = send.try_send(code);
            false
        })
        .map_err(io::Error::other)?;
    let mut options = PropMap::new();
    options.insert(
        "handle_token".into(),
        Variant(Box::new(HANDLE_TOKEN.to_owned())),
    );
    if let Some(body) = body {
        // D-Bus strings cannot contain U+0000.
        options.insert(
            "body".into(),
            Variant(Box::new(body.replace('\0', "\u{fffd}"))),
        );
    }
    if !attachments.is_empty() {
        let descriptors = attachments
            .iter()
            .map(|path| open_attachment(path))
            .collect::<io::Result<Vec<_>>>()?;
        options.insert("attachment_fds".into(), Variant(Box::new(descriptors)));
    }
    let portal = connection.with_proxy(PORTAL_BUS, PORTAL_PATH, Duration::from_secs(3));
    if !authorized() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "ShareAuthorizationExpired",
        ));
    }
    let (path,): (dbus::Path<'static>,) = portal
        .method_call(
            "org.freedesktop.portal.Email",
            "ComposeEmail",
            ("", options),
        )
        .map_err(io::Error::other)?;
    let request = connection.with_proxy(PORTAL_BUS, path.clone(), Duration::from_secs(2));
    if path == expected_path {
        let deadline = Instant::now() + Duration::from_secs(9);
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            if let Ok(code) = receive.try_recv() {
                return match code {
                    0 => Ok(true),
                    1 => Ok(false),
                    _ => Err(io::Error::other("ShareEmailUnavailable")),
                };
            }
            connection.process(remaining).map_err(io::Error::other)?;
        }
    }
    let _ = request.method_call::<(), _, _, _>(REQUEST_INTERFACE, "Close", ());
    Err(io::Error::other("ShareEmailUnconfirmed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbus::arg::messageitem::MessageItem;
    use dbus::channel::{MatchingReceiver, Sender};
    use dzc_core::api::{HistoryService, SensitiveAccess};
    use std::fs::File;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::fd::AsRawFd;
    use std::process::{Child, Command, Stdio};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct Bus(Child);

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    #[ignore = "requires a local dbus-daemon executable"]
    fn share_email_portal_accepts_body_and_files_and_reports_cancellation() {
        let mut config = tempfile::NamedTempFile::new().unwrap();
        // This bus may only serve the mock, even if the matching callback fails.
        write!(
            config,
            r#"<busconfig>
            <type>session</type><listen>unix:tmpdir=/tmp</listen>
            <policy context="default">
                <allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/>
            </policy>
        </busconfig>"#
        )
        .unwrap();
        let mut bus = Bus(Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.path().display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap());
        let mut address = String::new();
        BufReader::new(bus.0.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let connect = || Connection::new_address(address.trim()).unwrap();
        let portal = Connection::new_address(address.trim()).unwrap();
        portal.request_name(PORTAL_BUS, false, false, true).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let handled = calls.clone();
        portal.start_receive(
            MatchRule::new_method_call(),
            Box::new(move |message, conn| {
                assert_eq!(message.member().unwrap().to_string(), "ComposeEmail");
                let (parent, options): (String, PropMap) = message.read2().unwrap();
                assert!(parent.is_empty());
                assert_eq!(
                    dbus::arg::prop_cast::<String>(&options, "body").unwrap(),
                    "完整正文\nemoji 🙂"
                );
                assert!(!options.contains_key("address"));
                assert!(!options.contains_key("addresses"));
                let token = dbus::arg::prop_cast::<String>(&options, "handle_token").unwrap();
                let sender = message
                    .sender()
                    .unwrap()
                    .trim_start_matches(':')
                    .replace('.', "_");
                let path =
                    dbus::Path::new(format!("{PORTAL_PATH}/request/{sender}/{token}")).unwrap();
                let items = message.get_items();
                let MessageItem::Dict(values) = &items[1] else { panic!("missing options") };
                let (_, MessageItem::Variant(descriptors)) = values.iter()
                    .find(|(name, _)| matches!(name, MessageItem::Str(name) if name == "attachment_fds"))
                    .unwrap() else { panic!("missing attachment descriptors") };
                let MessageItem::Array(descriptors) = descriptors.as_ref() else { panic!("expected a descriptor array") };
                assert_eq!(descriptors.len(), 1);
                let descriptor: &OwnedFd = (&descriptors[0]).try_into().unwrap();
                let mut bytes = Vec::new();
                let mut attachment = File::from(descriptor.try_clone().unwrap());
                // The real email portal resolves the FD back to its existing filename.
                let attachment_path = std::fs::read_link(format!("/proc/self/fd/{}", attachment.as_raw_fd())).unwrap();
                assert_eq!(attachment_path.file_name().unwrap(), "capture.png");
                assert_eq!(std::fs::read(attachment_path).unwrap(), b"test attachment");
                attachment.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"test attachment");
                conn.send(message.method_return().append1(path.clone()))
                    .unwrap();
                let code = handled.fetch_add(1, Ordering::Relaxed) as u32;
                let response =
                    dbus::Message::new_signal(path.to_string(), REQUEST_INTERFACE, "Response")
                        .unwrap()
                        .append2(code, PropMap::new());
                conn.send(response).unwrap();
                true
            }),
        );
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(6);
            while calls.load(Ordering::Relaxed) < 2 {
                portal
                    .process(
                        deadline
                            .checked_duration_since(Instant::now())
                            .expect("mock share deadline"),
                    )
                    .unwrap();
            }
        });
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let entry = serde_json::from_value(serde_json::json!({
            "id":0, "content_type":"text", "content":"test attachment", "preview":"test attachment",
            "source_app":"Synthetic", "timestamp":1, "is_pinned":false,
        })).unwrap();
        let id = history.save(&entry, None).unwrap();
        let permit = history.privacy().authorize_selected_copy(id, None, SensitiveAccess::Denied, None, 1).unwrap();
        let mut export = history.external_files_authorized(id, &permit, 1).unwrap();
        export.files[0].name = "capture.png".into();
        let attachment = crate::runtime::private_email_attachment_export(export).unwrap();
        let paths = || attachment.paths().unwrap();
        let body = || Some("完整正文\nemoji 🙂".to_owned());
        let denied = compose_on(&connect(), body(), paths(), || false).unwrap_err();
        assert_eq!(denied.kind(), io::ErrorKind::PermissionDenied);
        assert!(compose_on(&connect(), body(), paths(), || true).unwrap());
        assert!(!compose_on(&connect(), body(), paths(), || true).unwrap());
        worker.join().unwrap();
        let path = paths().remove(0);
        drop(attachment);
        assert!(!std::path::Path::new(&path).exists());
    }
}
