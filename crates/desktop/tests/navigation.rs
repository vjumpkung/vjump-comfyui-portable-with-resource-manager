use desktop::comfy_window::{Navigation, browser_url_allowed, navigation};

#[test]
fn only_the_current_comfyui_origin_is_embedded() {
    let origin = "http://127.0.0.1:8188".parse().unwrap();
    for url in [
        "http://127.0.0.1:8188/",
        "http://127.0.0.1:8188/workflow?x=1#node",
    ] {
        assert_eq!(
            navigation(&origin, &url.parse().unwrap()),
            Navigation::Embedded
        );
    }
    for url in [
        "http://127.0.0.1:8189/",
        "http://localhost:8188/",
        "https://example.com/",
    ] {
        assert_eq!(
            navigation(&origin, &url.parse().unwrap()),
            Navigation::Browser
        );
    }
}

#[test]
fn credentials_and_executable_or_local_file_schemes_are_denied() {
    let origin = "http://127.0.0.1:8188".parse().unwrap();
    for text in [
        "javascript:alert(1)",
        "file:///C:/Windows/win.ini",
        "data:text/html,test",
        "tauri://localhost",
        "https://user:password@example.com/",
        "http://user@127.0.0.1:8188/",
    ] {
        let url = text.parse().unwrap();
        assert!(!browser_url_allowed(&url), "{text}");
        assert_eq!(navigation(&origin, &url), Navigation::Denied, "{text}");
    }
}
