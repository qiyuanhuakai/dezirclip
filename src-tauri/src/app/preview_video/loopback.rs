//! WebKitGTK's media stack requires an HTTP URL for seekable local video.
use std::{
    convert::Infallible, io, net::Ipv4Addr, os::unix::fs::FileExt, sync::Arc, time::Duration,
};

use futures_util::stream;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, StreamBody};
use hyper::{
    body::{Bytes, Frame, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use tauri::{
    http::{header, Method, Request, Response, StatusCode},
    AppHandle, Manager,
};
use tokio::{net::TcpListener, sync::Semaphore};

use super::{range_bounds, DesktopHost, Video, Videos};

const CHUNK_BYTES: usize = 128 * 1024;
type Body = UnsyncBoxBody<Bytes, io::Error>;

pub(super) async fn url(app: &AppHandle, token: &str) -> Result<String, String> {
    let videos = app.state::<Videos>();
    let port = videos
        .port
        .get_or_try_init(|| async {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = listener.local_addr()?;
            let app = app.clone();
            tauri::async_runtime::spawn(serve(listener, app, address.to_string()));
            Ok::<_, io::Error>(address.port())
        })
        .await
        .map_err(|_| "VideoPreviewUnavailable")?;
    Ok(format!("http://127.0.0.1:{port}/{token}"))
}

async fn serve(listener: TcpListener, app: AppHandle, authority: String) {
    let connections = Arc::new(Semaphore::new(16));
    while let Ok((socket, _)) = listener.accept().await {
        let Ok(permit) = connections.clone().try_acquire_owned() else {
            continue;
        };
        let (app, authority) = (app.clone(), authority.clone());
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let service = service_fn(move |request| {
                let response = respond(&app, &authority, request);
                async { Ok::<_, Infallible>(response) }
            });
            let _ = http1::Builder::new()
                .keep_alive(false)
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(5))
                .serve_connection(TokioIo::new(socket), service)
                .await;
        });
    }
}

fn empty() -> Body {
    Full::new(Bytes::new())
        .map_err(|error| match error {})
        .boxed_unsync()
}

fn respond(app: &AppHandle, authority: &str, request: Request<Incoming>) -> Response<Body> {
    let reply = |status| {
        Response::builder()
            .status(status)
            .header(header::CACHE_CONTROL, "no-store")
    };
    let head = request.method() == Method::HEAD;
    if !head && request.method() != Method::GET {
        return reply(StatusCode::METHOD_NOT_ALLOWED)
            .body(empty())
            .expect("static HTTP response");
    }
    let token = request.uri().path().strip_prefix('/').unwrap_or_default();
    if request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        != Some(authority)
        || token.len() != 32
        || !token.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return reply(StatusCode::NOT_FOUND)
            .body(empty())
            .expect("static HTTP response");
    }
    let Some((label, video)) = app.state::<Videos>().active.lock().ok().and_then(|active| {
        active
            .iter()
            .find(|(_, video)| video.token == token)
            .map(|(label, video)| (label.clone(), video.clone()))
    }) else {
        return reply(StatusCode::NOT_FOUND)
            .body(empty())
            .expect("static HTTP response");
    };
    let media = Media {
        app: app.clone(),
        label,
        video,
    };
    if !media.current() {
        return reply(StatusCode::GONE)
            .body(empty())
            .expect("static HTTP response");
    }

    let length = media.video.length;
    let mut builder = reply(StatusCode::OK)
        .header(header::CONTENT_TYPE, media.video.mime)
        .header(header::ACCEPT_RANGES, "bytes");
    let (start, end) = if let Some(range) = request.headers().get(header::RANGE) {
        let Some((start, end)) = range
            .to_str()
            .ok()
            .and_then(|range| range_bounds(range, length, u64::MAX))
        else {
            return builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{length}"))
                .body(empty())
                .expect("static range response");
        };
        builder = builder.status(StatusCode::PARTIAL_CONTENT).header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{length}"),
        );
        (start, end)
    } else {
        (0, length - 1)
    };
    let body = if head {
        empty()
    } else {
        media.body(start, end)
    };
    builder
        .header(header::CONTENT_LENGTH, end - start + 1)
        .body(body)
        .expect("validated media response")
}

struct Media {
    app: AppHandle,
    label: String,
    video: Arc<Video>,
}

impl Media {
    fn current(&self) -> bool {
        self.app
            .state::<DesktopHost>()
            .check_queue_state(&self.video.instance, self.video.epoch)
            .is_ok()
            && self
                .app
                .state::<Videos>()
                .active
                .lock()
                .is_ok_and(|active| {
                    active
                        .get(&self.label)
                        .is_some_and(|video| Arc::ptr_eq(video, &self.video))
                })
    }

    fn body(self, start: u64, end: u64) -> Body {
        let media = Arc::new(self);
        let stream = stream::try_unfold(start, move |offset| {
            let media = media.clone();
            async move {
                if offset > end {
                    return Ok(None);
                }
                let size = (end - offset + 1).min(CHUNK_BYTES as u64) as usize;
                let bytes = tauri::async_runtime::spawn_blocking(move || {
                    if !media.current() {
                        return Err(io::ErrorKind::PermissionDenied.into());
                    }
                    let mut bytes = vec![0; size];
                    media
                        .video
                        .file
                        .lock()
                        .map_err(|_| io::ErrorKind::Other)?
                        .read_exact_at(&mut bytes, offset)?;
                    if !media.current() {
                        return Err(io::ErrorKind::PermissionDenied.into());
                    }
                    Ok::<_, io::Error>(Bytes::from(bytes))
                })
                .await
                .map_err(io::Error::other)??;
                Ok(Some((Frame::data(bytes), offset + size as u64)))
            }
        });
        StreamBody::new(stream).boxed_unsync()
    }
}
