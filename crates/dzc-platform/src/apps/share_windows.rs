use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{factory, w, Interface, Ref, HSTRING},
    ApplicationModel::DataTransfer::{
        DataPackage, DataRequestedEventArgs, DataTransferManager, HtmlFormatHelper,
    },
    Foundation::{TypedEventHandler, Uri},
    Storage::{IStorageItem, StorageFile, Streams::RandomAccessStreamReference},
    Win32::{
        Foundation::{COLORREF, HWND, POINT},
        System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_SINGLETHREADED},
        UI::{
            Shell::IDataTransferManagerInterop,
            WindowsAndMessaging::{
                AllowSetForegroundWindow, CreateWindowExW, DestroyWindow, DispatchMessageW,
                GetCursorPos, MsgWaitForMultipleObjectsEx, PeekMessageW, SetForegroundWindow,
                SetLayeredWindowAttributes, ShowWindow, TranslateMessage, LWA_ALPHA, MSG,
                MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SW_SHOW, WS_EX_LAYERED,
                WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
};

pub enum WindowsShare {
    Text {
        text: String,
        html: Option<String>,
        link: bool,
    },
    Files {
        paths: Vec<String>,
        image: bool,
    },
}

enum Progress {
    Authorize(mpsc::SyncSender<bool>),
    Ready,
    Canceled,
    Failed(io::Error),
}

static ACTIVE: Mutex<Option<ShareSession>> = Mutex::new(None);

struct ShareSession {
    canceled: Arc<AtomicBool>,
    finished: mpsc::Receiver<()>,
}

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct ShareWindow(HWND);

impl ShareWindow {
    fn create() -> windows::core::Result<Self> {
        // Both frontends share through Host, which can be a separate desktop process.
        // The shell requires an active owner on the monitor where Share was requested.
        let mut cursor = POINT::default();
        unsafe {
            GetCursorPos(&mut cursor)?;
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("DezirClip"),
                WS_POPUP,
                cursor.x,
                cursor.y,
                160,
                120,
                None,
                None,
                None,
                None,
            )?;
            let window = Self(hwnd);
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA)?;
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            Ok(window)
        }
    }
}

/// Transfer foreground activation to the verified service that will show Share.
pub fn grant_windows_share_activation(pid: u32) -> io::Result<()> {
    if pid == 0 || pid == u32::MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "InvalidShareProcess",
        ));
    }
    unsafe { AllowSetForegroundWindow(pid) }.map_err(io::Error::other)
}

impl Drop for ShareWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

fn data_package(contents: WindowsShare) -> windows::core::Result<DataPackage> {
    let package = DataPackage::new()?;
    package
        .Properties()?
        .SetTitle(&HSTRING::from("DezirClip"))?;
    match contents {
        WindowsShare::Text { text, html, link } => {
            package.SetText(&HSTRING::from(&text))?;
            if let Some(html) = html.filter(|value| !value.is_empty()) {
                let formatted = HtmlFormatHelper::CreateHtmlFormat(&HSTRING::from(html))?;
                package.SetHtmlFormat(&formatted)?;
            }
            if link {
                if let Ok(uri) = url::Url::parse(&text) {
                    if matches!(uri.scheme(), "https" | "http") {
                        package.SetWebLink(&Uri::CreateUri(&HSTRING::from(uri.as_str()))?)?;
                    }
                }
            }
        }
        WindowsShare::Files { paths, image } => {
            let files = paths
                .into_iter()
                .map(|path| StorageFile::GetFileFromPathAsync(&HSTRING::from(path))?.join())
                .collect::<windows::core::Result<Vec<_>>>()?;
            let items: windows_collections::IIterable<IStorageItem> = files
                .iter()
                .map(|file| file.cast::<IStorageItem>().map(Some))
                .collect::<windows::core::Result<Vec<_>>>()?
                .into();
            package.SetStorageItemsReadOnly(&items)?;
            if image {
                if let [file] = files.as_slice() {
                    let bitmap = RandomAccessStreamReference::CreateFromFile(file)?;
                    package.SetBitmap(&bitmap)?;
                }
            }
        }
    }
    Ok(package)
}

fn event_loop(stop: impl Fn() -> bool, timeout: Duration) {
    let until = Instant::now() + timeout;
    while !stop() && Instant::now() < until {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            MsgWaitForMultipleObjectsEx(None, 50, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
}

fn present(
    contents: WindowsShare,
    progress: &mpsc::Sender<Progress>,
    canceled: Arc<AtomicBool>,
) -> windows::core::Result<()> {
    unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }?;
    let _apartment = Apartment;
    let package = data_package(contents)?;
    let window = ShareWindow::create()?;
    let interop = factory::<DataTransferManager, IDataTransferManagerInterop>()?;
    let manager: DataTransferManager = unsafe { interop.GetForWindow(window.0) }?;
    let closed = Arc::new(AtomicBool::new(false));
    let presented = Arc::new(AtomicBool::new(false));
    let on_cancel = closed.clone();
    let cancellation = package.ShareCanceled(&TypedEventHandler::new(move |_, _| {
        on_cancel.store(true, Ordering::Release);
        Ok(())
    }))?;
    let on_target = closed.clone();
    let target = manager.TargetApplicationChosen(&TypedEventHandler::new(move |_, _| {
        on_target.store(true, Ordering::Release);
        Ok(())
    }))?;
    let data = package.clone();
    let notify = progress.clone();
    let observed = presented.clone();
    let on_error = closed.clone();
    let superseded = canceled.clone();
    let requested = manager.DataRequested(&TypedEventHandler::new(
        move |_, args: Ref<DataRequestedEventArgs>| {
            let request = args.ok()?.Request()?;
            if superseded.load(Ordering::Acquire) || observed.swap(true, Ordering::AcqRel) {
                let _ = request.FailWithDisplayText(&HSTRING::from("DezirClip"));
                return Ok(());
            }
            let (send, receive) = mpsc::sync_channel(1);
            if notify.send(Progress::Authorize(send)).is_err()
                || receive.recv_timeout(Duration::from_secs(2)).ok() != Some(true)
            {
                let _ = request.FailWithDisplayText(&HSTRING::from("DezirClip"));
                on_error.store(true, Ordering::Release);
                let _ = notify.send(Progress::Failed(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "ShareAuthorizationExpired",
                )));
                return Ok(());
            }
            let result = request.SetData(&data).map_err(io::Error::other);
            if result.is_err() {
                on_error.store(true, Ordering::Release);
            }
            let _ = notify.send(match result {
                Ok(()) => Progress::Ready,
                Err(error) => Progress::Failed(error),
            });
            Ok(())
        },
    ))?;
    let shown = unsafe { interop.ShowShareUIForWindow(window.0) };
    if shown.is_ok() {
        let shown_at = Instant::now();
        event_loop(
            || {
                closed.load(Ordering::Acquire)
                    || canceled.load(Ordering::Acquire)
                    || !presented.load(Ordering::Acquire)
                        && shown_at.elapsed() >= Duration::from_secs(12)
            },
            Duration::from_secs(90),
        );
    }
    let _ = manager.RemoveDataRequested(requested);
    let _ = manager.RemoveTargetApplicationChosen(target);
    let _ = package.RemoveShareCanceled(cancellation);
    if shown.is_ok() && !presented.load(Ordering::Acquire) {
        let outcome = if closed.load(Ordering::Acquire) {
            Progress::Canceled
        } else {
            Progress::Failed(io::Error::new(io::ErrorKind::TimedOut, "ShareUnconfirmed"))
        };
        let _ = progress.send(outcome);
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::ApplicationModel::DataTransfer::StandardDataFormats;

    #[test]
    fn windows_share_packages_complete_text_html_links_and_files() {
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }.unwrap();
        let _apartment = Apartment;
        let text = "https://example.com/中文";
        let package = data_package(WindowsShare::Text {
            text: text.into(),
            html: Some("<b>完整文字</b>".into()),
            link: true,
        })
        .unwrap();
        let view = package.GetView().unwrap();
        assert_eq!(
            view.GetTextAsync().unwrap().join().unwrap().to_string(),
            text
        );
        let html = view
            .GetHtmlFormatAsync()
            .unwrap()
            .join()
            .unwrap()
            .to_string();
        assert!(html.contains("<b>完整文字</b>"));
        assert!(view
            .Contains(&StandardDataFormats::WebLink().unwrap())
            .unwrap());

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("中文附件.txt");
        std::fs::write(&path, b"Windows Share fixture").unwrap();
        let package = data_package(WindowsShare::Files {
            paths: vec![path.to_string_lossy().into_owned()],
            image: false,
        })
        .unwrap();
        let items = package
            .GetView()
            .unwrap()
            .GetStorageItemsAsync()
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(items.Size().unwrap(), 1);
        assert_eq!(
            items.GetAt(0).unwrap().Name().unwrap().to_string(),
            "中文附件.txt"
        );
        let path = directory.path().join("capture.png");
        std::fs::write(&path, include_bytes!("dezirclip.png")).unwrap();
        let package = data_package(WindowsShare::Files {
            paths: vec![path.to_string_lossy().into_owned()],
            image: true,
        })
        .unwrap();
        let view = package.GetView().unwrap();
        assert!(view
            .Contains(&StandardDataFormats::Bitmap().unwrap())
            .unwrap());
        assert!(view
            .Contains(&StandardDataFormats::StorageItems().unwrap())
            .unwrap());
    }

    #[test]
    #[ignore = "opens the Windows share sheet on the current desktop"]
    fn windows_share_sheet_requests_the_package() {
        let interactive = std::env::var_os("DZC_SHARE_INTERACTIVE").is_some();
        for attempt in 1..=if interactive { 2 } else { 1 } {
            if interactive {
                use windows::Win32::UI::WindowsAndMessaging::{
                    MessageBoxW, IDOK, MB_OKCANCEL, MB_SETFOREGROUND,
                };
                let question = HSTRING::from(format!(
                    "Open Windows Share with synthetic text ({attempt}/2)?"
                ));
                let confirmed = unsafe {
                    MessageBoxW(
                        None,
                        windows::core::PCWSTR(question.as_ptr()),
                        w!("DezirClip Share smoke test"),
                        MB_OKCANCEL | MB_SETFOREGROUND,
                    )
                };
                assert_eq!(confirmed, IDOK);
            }
            assert!(show_windows_share(
                WindowsShare::Text {
                    text: format!("DezirClip Windows Share test {attempt}"),
                    html: None,
                    link: false,
                },
                || true
            )
            .unwrap());
        }
    }
}

/// Hand the selected entry to the Windows Share sheet after checking its authorization.
pub fn show_windows_share(
    contents: WindowsShare,
    authorized: impl FnOnce() -> bool,
) -> io::Result<bool> {
    let busy = || io::Error::new(io::ErrorKind::WouldBlock, "ShareInProgress");
    let mut active = ACTIVE.try_lock().map_err(|_| busy())?;
    if let Some(previous) = active.as_ref() {
        // Retire the previous owner even if the shell did not report ShareCanceled.
        previous.canceled.store(true, Ordering::Release);
        if matches!(
            previous.finished.recv_timeout(Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            return Err(busy());
        }
    }
    let (progress, receiver) = mpsc::channel();
    let (finished, completion) = mpsc::channel();
    let canceled = Arc::new(AtomicBool::new(false));
    let stop = canceled.clone();
    std::thread::Builder::new()
        .name("windows-share".into())
        .spawn(move || {
            if let Err(error) = present(contents, &progress, stop) {
                let _ = progress.send(Progress::Failed(io::Error::other(error)));
            }
            let _ = finished.send(());
        })?;
    *active = Some(ShareSession {
        canceled,
        finished: completion,
    });
    let mut authorized = Some(authorized);
    let deadline = Instant::now() + Duration::from_secs(12);
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match receiver.recv_timeout(remaining) {
            Ok(Progress::Authorize(reply)) => {
                let _ = reply.send(authorized.take().is_some_and(|check| check()));
            }
            Ok(Progress::Ready) => return Ok(true),
            Ok(Progress::Canceled) => return Ok(false),
            Ok(Progress::Failed(error)) => return Err(error),
            Err(_) => break,
        }
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "ShareUnconfirmed"))
}
