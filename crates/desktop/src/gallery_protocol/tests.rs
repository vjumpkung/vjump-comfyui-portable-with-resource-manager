use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use application::{
    environment::{RootInfo, RootSource, RootSwitchError, RootSwitcher},
    gallery::{GalleryDeletePlans, GalleryDeps},
};
use async_trait::async_trait;
use infrastructure::{
    gallery::files::LocalGalleryFiles,
    shared::{clock::RandomIds, gate::TokioMaintenanceGate},
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

use super::*;
use crate::comfy_window::COMFYUI_WINDOW;

fn from(start: u64, end: Option<u64>) -> Option<ByteRange> {
    Some(ByteRange::From { start, end })
}

#[test]
fn single_byte_ranges_are_parsed() {
    for (header, expected) in [
        ("bytes=0-0", from(0, Some(0))),
        ("bytes=100-199", from(100, Some(199))),
        ("bytes=100-", from(100, None)),
        ("bytes=-500", Some(ByteRange::Suffix(500))),
        ("bytes=-0", Some(ByteRange::Suffix(0))),
        (" bytes=1-2 ", from(1, Some(2))),
        ("bytes= 1 - 2 ", from(1, Some(2))),
        // Ignored: the whole file is served instead.
        ("bytes=0-1,5-6", None),
        ("bytes=5-2", None),
        ("bytes=-", None),
        ("bytes=", None),
        ("bytes=a-b", None),
        ("bytes=+1-2", None),
        ("bytes=1-2-3", None),
        ("bytes=99999999999999999999-", None),
        ("items=0-1", None),
        ("0-1", None),
        ("", None),
    ] {
        assert_eq!(parse_range(header), expected, "{header:?}");
    }
}

#[test]
fn ranges_resolve_against_the_length_with_the_partial_cap() {
    const CAP: u64 = MAX_RANGE_BYTES;
    for (range, len, expected) in [
        (from(0, Some(9)), 100, Some((0, 9))),
        (from(0, Some(500)), 100, Some((0, 99))),
        (from(99, None), 100, Some((99, 99))),
        (from(100, None), 100, None),
        (from(100, Some(120)), 100, None),
        (from(0, None), 0, None),
        (Some(ByteRange::Suffix(10)), 100, Some((90, 99))),
        (Some(ByteRange::Suffix(500)), 100, Some((0, 99))),
        (Some(ByteRange::Suffix(0)), 100, None),
        (Some(ByteRange::Suffix(5)), 0, None),
        (from(0, None), CAP + 10, Some((0, CAP - 1))),
        (from(5, Some(CAP * 4)), CAP * 8, Some((5, CAP + 4))),
        (
            Some(ByteRange::Suffix(CAP * 2)),
            CAP * 3,
            Some((CAP, CAP * 2 - 1)),
        ),
        (
            from(u64::MAX - 1, None),
            u64::MAX,
            Some((u64::MAX - 1, u64::MAX - 1)),
        ),
    ] {
        assert_eq!(resolve(range.unwrap(), len), expected, "{range:?} of {len}");
    }
}

#[test]
fn locations_are_decoded_once_per_segment() {
    let thai = "ภาพ ทดสอบ.png";
    let encoded = utf8_percent_encode(thai, NON_ALPHANUMERIC).to_string();
    let location = location_of(&format!("/output/2026/{encoded}")).unwrap();
    assert_eq!(location.area(), GalleryArea::Output);
    assert_eq!(location.path().as_string(), format!("2026/{thai}"));

    let spaces = location_of("/input/my%20folder/a%20b.png").unwrap();
    assert_eq!(spaces.area(), GalleryArea::Input);
    assert_eq!(spaces.path().as_string(), "my folder/a b.png");

    // Decoding twice would turn this into `a b.png`; it stays a literal percent sign.
    let once = location_of("/input/a%2520b.png").unwrap();
    assert_eq!(once.path().as_string(), "a%20b.png");
    assert_eq!(
        location_of("/input/plain.png").unwrap().path().as_string(),
        "plain.png"
    );
}

#[test]
fn hostile_or_unaddressable_paths_are_refused() {
    use StatusCode as S;
    for (path, status) in [
        ("/input/a%2Fb.png", S::BAD_REQUEST),
        ("/input/a%2fb.png", S::BAD_REQUEST),
        ("/input/a%5Cb.png", S::BAD_REQUEST),
        ("/input/a%5cb.png", S::BAD_REQUEST),
        ("/input/..", S::BAD_REQUEST),
        ("/input/%2e%2e/secret.png", S::BAD_REQUEST),
        ("/input/a/../b.png", S::BAD_REQUEST),
        ("/input/%2E%2E%2Fsecret.png", S::BAD_REQUEST),
        ("/input/.vjump-gallery-staging/x.png", S::BAD_REQUEST),
        ("/output/a/.VJUMP-Gallery-Staging", S::BAD_REQUEST),
        ("/output/%2Evjump-gallery-staging/x.png", S::BAD_REQUEST),
        ("/input/%FF.png", S::BAD_REQUEST),
        ("/input/%E0%B8.png", S::BAD_REQUEST),
        ("/input/a//b.png", S::BAD_REQUEST),
        ("/input/a/", S::BAD_REQUEST),
        ("/input/C%3A.png", S::BAD_REQUEST),
        ("/input/a%00b.png", S::BAD_REQUEST),
        // The area root is not a file.
        ("/input", S::BAD_REQUEST),
        ("/input/", S::BAD_REQUEST),
        ("/", S::NOT_FOUND),
        ("", S::BAD_REQUEST),
        ("/models/a.png", S::NOT_FOUND),
        ("/INPUT/a.png", S::NOT_FOUND),
        ("/inp%75t/a.png", S::NOT_FOUND),
    ] {
        assert_eq!(location_of(path), Err(status), "{path:?}");
    }
}

#[test]
fn content_types_follow_the_lowercase_extension() {
    for (name, expected) in [
        ("a.png", "image/png"),
        ("a.PNG", "image/png"),
        ("a.jpg", "image/jpeg"),
        ("a.JPEG", "image/jpeg"),
        ("a.webp", "image/webp"),
        ("a.gif", "image/gif"),
        ("a.bmp", "image/bmp"),
        ("a.avif", "image/avif"),
        ("a.mp4", "video/mp4"),
        ("a.m4v", "video/mp4"),
        ("a.webm", "video/webm"),
        ("a.mov", "video/quicktime"),
        ("a.glb", "model/gltf-binary"),
        ("a.gltf", "model/gltf+json"),
        ("ภาพ.Png", "image/png"),
        ("clip.final.webm", "video/webm"),
        ("a.svg", "application/octet-stream"),
        ("a.html", "application/octet-stream"),
        ("a.obj", "application/octet-stream"),
        ("png", "application/octet-stream"),
        (".png", "application/octet-stream"),
        ("noextension", "application/octet-stream"),
    ] {
        assert_eq!(content_type(name), expected, "{name}");
    }
}

struct FixedRoot(PathBuf);
#[async_trait]
impl RootSwitcher for FixedRoot {
    fn current(&self) -> RootInfo {
        RootInfo {
            root: self.0.clone(),
            source: RootSource::Default,
        }
    }
    async fn switch_root(&self, _: &Path, _: Option<&str>) -> Result<(), RootSwitchError> {
        Ok(())
    }
}

/// A temporary install tree with real gallery adapters.
struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    open: OpenGalleryMedia,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("install root ไทย");
        fs::create_dir_all(root.join("ComfyUI/input")).unwrap();
        fs::create_dir_all(root.join("ComfyUI/output")).unwrap();
        let open = OpenGalleryMedia::new(GalleryDeps {
            root: Arc::new(FixedRoot(root.clone())),
            files: Arc::new(LocalGalleryFiles::new()),
            gate: Arc::new(TokioMaintenanceGate::new()),
            ids: Arc::new(RandomIds::default()),
            plans: Arc::new(GalleryDeletePlans::default()),
        });
        Self {
            _directory: directory,
            root,
            open,
        }
    }

    /// `relative` is below `ComfyUI/`, e.g. `input/a.png`.
    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.root.join("ComfyUI").join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn send(
        &self,
        label: &str,
        method: &str,
        path: &str,
        range: Option<&str>,
    ) -> http::Response<Vec<u8>> {
        let mut request = http::Request::builder()
            .method(method)
            .uri(format!("http://gallery.localhost{path}"));
        if let Some(range) = range {
            request = request.header(header::RANGE, range);
        }
        let request = request.body(Vec::new()).unwrap();
        tauri::async_runtime::block_on(serve(&self.open, label, &request))
    }

    fn get(&self, path: &str, range: Option<&str>) -> http::Response<Vec<u8>> {
        self.send(MANAGER_WINDOW, "GET", path, range)
    }
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index % 251) as u8).collect()
}

fn text(response: &http::Response<Vec<u8>>, name: HeaderName) -> &str {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().unwrap())
        .unwrap_or_default()
}

fn assert_data_headers(response: &http::Response<Vec<u8>>, content_type: &str) {
    assert_eq!(text(response, header::CONTENT_TYPE), content_type);
    assert_eq!(text(response, header::ACCEPT_RANGES), "bytes");
    assert_eq!(text(response, header::X_CONTENT_TYPE_OPTIONS), "nosniff");
    assert_eq!(text(response, header::CACHE_CONTROL), "no-cache");
    assert_eq!(text(response, header::CONTENT_SECURITY_POLICY), "sandbox");
    assert_eq!(text(response, header::ACCESS_CONTROL_ALLOW_ORIGIN), "*");
    assert_eq!(
        text(response, header::ACCESS_CONTROL_EXPOSE_HEADERS),
        "Content-Length, Content-Range, Accept-Ranges"
    );
}

#[test]
fn a_file_is_served_whole_by_its_percent_encoded_thai_name() {
    let fixture = Fixture::new();
    let data = pattern(10_000);
    fixture.write("input/โฟลเดอร์/ภาพ ทดสอบ.png", &data);
    let encoded = format!(
        "/input/{}/{}?v=123",
        utf8_percent_encode("โฟลเดอร์", NON_ALPHANUMERIC),
        utf8_percent_encode("ภาพ ทดสอบ.png", NON_ALPHANUMERIC)
    );
    let response = fixture.get(&encoded, None);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.body(), &data);
    assert_eq!(text(&response, header::CONTENT_LENGTH), "10000");
    assert!(response.headers().get(header::CONTENT_RANGE).is_none());
    assert_data_headers(&response, "image/png");
}

#[test]
fn a_range_gets_exactly_those_bytes() {
    let fixture = Fixture::new();
    let data = pattern(10_000);
    fixture.write("output/clip.MP4", &data);
    for (range, start, end) in [
        ("bytes=100-199", 100, 199),
        ("bytes=9990-", 9990, 9999),
        ("bytes=-5", 9995, 9999),
        ("bytes=0-0", 0, 0),
        ("bytes=9000-99999", 9000, 9999),
    ] {
        let response = fixture.get("/output/clip.MP4", Some(range));
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(response.body(), &data[start..=end], "{range}");
        assert_eq!(
            text(&response, header::CONTENT_RANGE),
            format!("bytes {start}-{end}/10000"),
            "{range}"
        );
        assert_eq!(
            text(&response, header::CONTENT_LENGTH),
            (end - start + 1).to_string(),
            "{range}"
        );
        assert_data_headers(&response, "video/mp4");
    }
}

#[test]
fn an_open_ended_range_is_served_in_capped_pieces() {
    let fixture = Fixture::new();
    let size = MAX_RANGE_BYTES as usize + 1024;
    let data = pattern(size);
    fixture.write("output/big.webm", &data);
    let first = fixture.get("/output/big.webm", Some("bytes=0-"));
    assert_eq!(first.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(first.body().len() as u64, MAX_RANGE_BYTES);
    assert_eq!(first.body()[..], data[..MAX_RANGE_BYTES as usize]);
    assert_eq!(
        text(&first, header::CONTENT_RANGE),
        format!("bytes 0-{}/{size}", MAX_RANGE_BYTES - 1)
    );
    let rest = fixture.get(
        "/output/big.webm",
        Some(&format!("bytes={MAX_RANGE_BYTES}-")),
    );
    assert_eq!(rest.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(rest.body()[..], data[MAX_RANGE_BYTES as usize..]);
    assert_eq!(
        text(&rest, header::CONTENT_RANGE),
        format!("bytes {MAX_RANGE_BYTES}-{}/{size}", size - 1)
    );
}

#[test]
fn an_unsatisfiable_range_is_416_with_the_length() {
    let fixture = Fixture::new();
    fixture.write("input/a.png", &pattern(100));
    fixture.write("input/empty.png", b"");
    for (path, range) in [
        ("/input/a.png", "bytes=100-"),
        ("/input/a.png", "bytes=500-600"),
        ("/input/a.png", "bytes=-0"),
        ("/input/empty.png", "bytes=0-0"),
        ("/input/empty.png", "bytes=-5"),
    ] {
        let response = fixture.get(path, Some(range));
        assert_eq!(
            response.status(),
            StatusCode::RANGE_NOT_SATISFIABLE,
            "{path} {range}"
        );
        assert!(response.body().is_empty());
        let len = if path.ends_with("empty.png") { 0 } else { 100 };
        assert_eq!(
            text(&response, header::CONTENT_RANGE),
            format!("bytes */{len}"),
            "{path} {range}"
        );
        assert_eq!(text(&response, header::CONTENT_LENGTH), "0");
        assert_data_headers(&response, "image/png");
    }
}

#[test]
fn a_range_the_server_cannot_read_serves_the_whole_file() {
    let fixture = Fixture::new();
    let data = pattern(300);
    fixture.write("input/a.gif", &data);
    for range in ["bytes=0-1,5-6", "bytes=7-3", "bytes=abc", "items=0-5"] {
        let response = fixture.get("/input/a.gif", Some(range));
        assert_eq!(response.status(), StatusCode::OK, "{range}");
        assert_eq!(response.body(), &data, "{range}");
    }
    // A file that is empty has nothing to range over, but is still served whole.
    fixture.write("input/empty.gif", b"");
    let response = fixture.get("/input/empty.gif", None);
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.body().is_empty());
    assert_eq!(text(&response, header::CONTENT_LENGTH), "0");
}

#[test]
fn head_carries_the_headers_of_get_without_a_body() {
    let fixture = Fixture::new();
    fixture.write("input/a.glb", &pattern(2_000));
    let whole = fixture.send(MANAGER_WINDOW, "HEAD", "/input/a.glb", None);
    assert_eq!(whole.status(), StatusCode::OK);
    assert!(whole.body().is_empty());
    assert_eq!(text(&whole, header::CONTENT_LENGTH), "2000");
    assert_data_headers(&whole, "model/gltf-binary");

    let partial = fixture.send(MANAGER_WINDOW, "HEAD", "/input/a.glb", Some("bytes=10-19"));
    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
    assert!(partial.body().is_empty());
    assert_eq!(text(&partial, header::CONTENT_LENGTH), "10");
    assert_eq!(text(&partial, header::CONTENT_RANGE), "bytes 10-19/2000");
}

#[test]
fn only_the_manager_window_may_read_gallery_files() {
    let fixture = Fixture::new();
    fixture.write("input/a.png", &pattern(10));
    for label in [COMFYUI_WINDOW, "untrusted", ""] {
        for method in ["GET", "HEAD", "OPTIONS", "POST"] {
            let response = fixture.send(label, method, "/input/a.png", None);
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{label}/{method}");
            assert!(response.body().is_empty());
        }
    }
}

#[test]
fn failures_are_bodiless_and_never_name_a_path() {
    let fixture = Fixture::new();
    fixture.write("input/folder/a.png", &pattern(10));
    for (path, status) in [
        ("/input/missing.png", StatusCode::NOT_FOUND),
        ("/input/folder/missing.png", StatusCode::NOT_FOUND),
        ("/input/nope/a.png", StatusCode::NOT_FOUND),
        ("/models/a.png", StatusCode::NOT_FOUND),
        // A folder is not a preview.
        ("/input/folder", StatusCode::BAD_REQUEST),
        ("/input", StatusCode::BAD_REQUEST),
        ("/input/a%2Fb.png", StatusCode::BAD_REQUEST),
        ("/input/.vjump-gallery-staging/x", StatusCode::BAD_REQUEST),
    ] {
        let response = fixture.get(path, None);
        assert_eq!(response.status(), status, "{path}");
        assert!(response.body().is_empty(), "{path}");
        assert_eq!(text(&response, header::X_CONTENT_TYPE_OPTIONS), "nosniff");
    }
}

#[test]
fn a_missing_install_folder_is_not_found() {
    let fixture = Fixture::new();
    fs::remove_dir_all(fixture.root.join("ComfyUI")).unwrap();
    let response = fixture.get("/output/a.png", None);
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(response.body().is_empty());
    assert!(!fixture.root.join("ComfyUI").exists(), "nothing is created");
}

#[test]
fn only_get_head_and_options_are_served() {
    let fixture = Fixture::new();
    fixture.write("input/a.png", &pattern(10));
    for method in ["POST", "PUT", "DELETE", "PATCH", "TRACE"] {
        let response = fixture.send(MANAGER_WINDOW, method, "/input/a.png", None);
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method}"
        );
        assert!(response.body().is_empty());
    }
    let preflight = fixture.send(MANAGER_WINDOW, "OPTIONS", "/input/a.png", None);
    assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
    assert!(preflight.body().is_empty());
    assert_eq!(text(&preflight, header::ACCESS_CONTROL_ALLOW_ORIGIN), "*");
    assert_eq!(
        text(&preflight, header::ACCESS_CONTROL_ALLOW_METHODS),
        "GET, HEAD"
    );
    assert_eq!(
        text(&preflight, header::ACCESS_CONTROL_ALLOW_HEADERS),
        "Range"
    );
    // The preflight does not depend on the file.
    let unknown = fixture.send(MANAGER_WINDOW, "OPTIONS", "/input/missing.png", None);
    assert_eq!(unknown.status(), StatusCode::NO_CONTENT);
}
