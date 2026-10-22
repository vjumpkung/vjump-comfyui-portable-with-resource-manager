//! The `gallery` preview protocol: read-only access to gallery files for the manager window's
//! `<img>`, `<video>` and model viewers. A request is `<scheme>/<area>/<path>`; the optional query
//! (`?v=`) only busts the webview cache.
//!
//! Custom protocols are not covered by the IPC capability ACL, so the requesting webview is
//! checked here: only the manager window may read gallery files, never the ComfyUI window. Files
//! are opened through the same pinned, non-following adapter the commands use, and every answer
//! is bodiless on failure and free of paths.

use std::io;

use application::gallery::{GalleryErrorCode, GalleryMedia, OpenGalleryMedia};
use domain::{
    gallery::{GalleryArea, GalleryLocation},
    shared::model_values::RelativeModelPath,
};
use percent_encoding::percent_decode_str;
use tauri::{
    Manager, Runtime, UriSchemeContext, UriSchemeResponder,
    http::{
        self, HeaderValue, Method, StatusCode,
        header::{self, HeaderName},
    },
};

use crate::{MANAGER_WINDOW, state::AppState};

/// The most one partial response carries; the player asks again for the rest.
const MAX_RANGE_BYTES: u64 = 8 * 1024 * 1024;
/// The largest file served whole, for a request without a `Range` header.
const MAX_FULL_BYTES: u64 = 256 * 1024 * 1024;

/// Answers one request on the async runtime. Recovery mode has no gallery services, so it answers
/// 503.
pub fn handle<R: Runtime>(
    context: UriSchemeContext<'_, R>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let label = context.webview_label().to_owned();
    let app = context.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let response = match app.try_state::<AppState>() {
            Some(state) => match state.services() {
                Ok(services) => serve(&services.gallery.open_media, &label, &request).await,
                Err(_) => empty(StatusCode::SERVICE_UNAVAILABLE),
            },
            None => empty(StatusCode::SERVICE_UNAVAILABLE),
        };
        responder.respond(response);
    });
}

pub async fn serve(
    open: &OpenGalleryMedia,
    webview_label: &str,
    request: &http::Request<Vec<u8>>,
) -> http::Response<Vec<u8>> {
    if webview_label != MANAGER_WINDOW {
        return empty(StatusCode::FORBIDDEN);
    }
    match *request.method() {
        Method::OPTIONS => return preflight(),
        Method::GET | Method::HEAD => {}
        _ => return empty(StatusCode::METHOD_NOT_ALLOWED),
    }
    let head = request.method() == Method::HEAD;
    let location = match location_of(request.uri().path()) {
        Ok(location) => location,
        Err(status) => return empty(status),
    };
    let media = match open.execute(&location).await {
        Ok(media) => media,
        Err(error) => return empty(status_of(error.0)),
    };
    let len = media.len();
    let content_type = location
        .name()
        .map_or(OCTET_STREAM, |name| content_type(name.as_str()));
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_range);
    let (status, start, length, content_range) = match range {
        Some(range) => match resolve(range, len) {
            Some((start, end)) => (
                StatusCode::PARTIAL_CONTENT,
                start,
                end - start + 1,
                Some(format!("bytes {start}-{end}/{len}")),
            ),
            None => {
                let mut response = media_response(
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    content_type,
                    0,
                    Vec::new(),
                );
                set(
                    &mut response,
                    header::CONTENT_RANGE,
                    &format!("bytes */{len}"),
                );
                return response;
            }
        },
        None if len > MAX_FULL_BYTES => return empty(StatusCode::PAYLOAD_TOO_LARGE),
        None => (StatusCode::OK, 0, len, None),
    };
    let body = if head {
        Vec::new()
    } else {
        match read_range(media, start, length).await {
            // A file that shrank since it was opened cannot fill the promised length.
            Ok(bytes) if bytes.len() as u64 == length => bytes,
            _ => return empty(StatusCode::INTERNAL_SERVER_ERROR),
        }
    };
    let mut response = media_response(status, content_type, length, body);
    if let Some(content_range) = content_range {
        set(&mut response, header::CONTENT_RANGE, &content_range);
    }
    response
}

fn status_of(code: GalleryErrorCode) -> StatusCode {
    match code {
        GalleryErrorCode::NotFound | GalleryErrorCode::NotInstalled => StatusCode::NOT_FOUND,
        GalleryErrorCode::InvalidRequest | GalleryErrorCode::InvalidName => StatusCode::BAD_REQUEST,
        GalleryErrorCode::AccessDenied => StatusCode::FORBIDDEN,
        GalleryErrorCode::Locked => StatusCode::LOCKED,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// `/<area>/<segment>/...` with every segment percent-decoded exactly once. The area root and
/// anything that is not a valid gallery location are refused.
fn location_of(path: &str) -> Result<GalleryLocation, StatusCode> {
    let rest = path.strip_prefix('/').ok_or(StatusCode::BAD_REQUEST)?;
    let (area, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let area = match area {
        "input" => GalleryArea::Input,
        "output" => GalleryArea::Output,
        _ => return Err(StatusCode::NOT_FOUND),
    };
    if tail.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut segments = Vec::new();
    for segment in tail.split('/') {
        let decoded = percent_decode_str(segment)
            .decode_utf8()
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        if decoded.contains(['/', '\\']) {
            return Err(StatusCode::BAD_REQUEST);
        }
        segments.push(decoded.into_owned());
    }
    let path = RelativeModelPath::new(&segments.join("/")).map_err(|_| StatusCode::BAD_REQUEST)?;
    GalleryLocation::new(area, path).map_err(|_| StatusCode::BAD_REQUEST)
}

/// The single byte range of a `Range` header. Anything else is ignored by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteRange {
    /// `bytes=a-b` and `bytes=a-`.
    From { start: u64, end: Option<u64> },
    /// `bytes=-n`: the last `n` bytes.
    Suffix(u64),
}

/// Multiple ranges, other units and malformed values yield `None`, which serves the whole file.
fn parse_range(value: &str) -> Option<ByteRange> {
    let spec = value.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (first, last) = spec.split_once('-')?;
    match (first.trim(), last.trim()) {
        ("", "") => None,
        ("", count) => Some(ByteRange::Suffix(number(count)?)),
        (start, "") => Some(ByteRange::From {
            start: number(start)?,
            end: None,
        }),
        (start, end) => {
            let (start, end) = (number(start)?, number(end)?);
            (start <= end).then_some(ByteRange::From {
                start,
                end: Some(end),
            })
        }
    }
}

fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The inclusive bytes to serve, at most [`MAX_RANGE_BYTES`] of them. `None` when the range
/// starts at or past the end of the file (or the file is empty).
fn resolve(range: ByteRange, len: u64) -> Option<(u64, u64)> {
    let last = len.checked_sub(1)?;
    let (start, end) = match range {
        ByteRange::From { start, end } => (start, end.map_or(last, |end| end.min(last))),
        ByteRange::Suffix(count) => (len.saturating_sub(count), last),
    };
    if start > last || matches!(range, ByteRange::Suffix(0)) {
        return None;
    }
    Some((start, end.min(start.saturating_add(MAX_RANGE_BYTES - 1))))
}

const OCTET_STREAM: &str = "application/octet-stream";

fn content_type(name: &str) -> &'static str {
    let extension = match name.rfind('.') {
        Some(0) | None => return OCTET_STREAM,
        Some(index) => name[index + 1..].to_ascii_lowercase(),
    };
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        _ => OCTET_STREAM,
    }
}

/// Reads `length` bytes at `start` on a blocking thread, stopping early only at end of file.
async fn read_range(
    mut media: Box<dyn GalleryMedia>,
    start: u64,
    length: u64,
) -> io::Result<Vec<u8>> {
    tauri::async_runtime::spawn_blocking(move || {
        let wanted = usize::try_from(length).map_err(|_| io::ErrorKind::InvalidInput)?;
        let mut buffer = vec![0u8; wanted];
        let mut filled = 0;
        while filled < wanted {
            match media.read_at(start + filled as u64, &mut buffer[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        buffer.truncate(filled);
        Ok(buffer)
    })
    .await
    .map_err(|_| io::Error::other("the preview read was interrupted"))?
}

fn set<B>(response: &mut http::Response<B>, name: HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        response.headers_mut().insert(name, value);
    }
}

/// The headers every answer carries: the file is data, never a document, and the manager page
/// (another origin) may read it.
fn base(status: StatusCode, body: Vec<u8>) -> http::Response<Vec<u8>> {
    let mut response = http::Response::new(body);
    *response.status_mut() = status;
    for (name, value) in [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::CACHE_CONTROL, "no-cache"),
        (header::CONTENT_SECURITY_POLICY, "sandbox"),
        (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        (
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Length, Content-Range, Accept-Ranges",
        ),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

/// A failure: no body, never a path.
fn empty(status: StatusCode) -> http::Response<Vec<u8>> {
    base(status, Vec::new())
}

/// A 2xx or 416 answer. `length` is the would-be body length, also for HEAD.
fn media_response(
    status: StatusCode,
    content_type: &'static str,
    length: u64,
    body: Vec<u8>,
) -> http::Response<Vec<u8>> {
    let mut response = base(status, body);
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    response
}

fn preflight() -> http::Response<Vec<u8>> {
    let mut response = base(StatusCode::NO_CONTENT, Vec::new());
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Range"),
    );
    response
}

#[cfg(test)]
mod tests;
