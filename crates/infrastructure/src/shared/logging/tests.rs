use std::sync::Arc;

use super::*;

#[test]
fn redacts_registered_values_and_preserves_clean_borrowed_text() {
    let redactor = Redactor::default();
    redactor.register_secret("short");
    redactor.register_secret("ไทย");
    assert!(matches!(
        redactor.redact("clean ไทย short"),
        Cow::Borrowed(_)
    ));
    redactor.register_secret("registered-secret");
    redactor.register_secret("registered-secret-longer");
    assert_eq!(
        redactor.redact("ไทย registered-secret-longer end"),
        "ไทย [REDACTED] end"
    );
    assert_eq!(
        redactor.redact("registered-secret twice registered-secret"),
        "[REDACTED] twice [REDACTED]"
    );
}

#[test]
fn redacts_headers_bearer_and_hugging_face_tokens() {
    for input in [
        "Authorization: Basic cHJpdmF0ZQ==\r\nNext: safe",
        "authorization: Bearer private-value\n",
        "Bearer standalone-value",
        "Bearer\ttab-private-value",
        "hf_01234567890123456789",
    ] {
        let output = redact(input);
        assert!(output.contains("[REDACTED]"));
        for value in [
            "cHJpdmF0ZQ==",
            "private-value",
            "standalone-value",
            "tab-private-value",
            "hf_01234567890123456789",
        ] {
            assert!(!output.contains(value));
        }
    }
    assert_eq!(redact("hf_short"), "hf_short");
}

#[test]
fn redacts_all_signed_query_parameters_without_removing_safe_parameters() {
    let input = "https://example.test/?safe=yes&token=secret-one&access_token=secret-two&api_key=secret-three&X-Amz-Signature=signature-value&X-Amz-Credential=credential-value&X-Amz-Date=date-value&X-Amz-Security-Token=session-value#fragment";
    let output = redact(input);
    assert!(output.contains("safe=yes"));
    assert!(output.ends_with("#fragment"));
    assert_eq!(output.matches("[REDACTED]").count(), 7);
    for value in [
        "secret-one",
        "secret-two",
        "secret-three",
        "signature-value",
        "credential-value",
        "date-value",
        "session-value",
    ] {
        assert!(!output.contains(value));
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self {
        self.clone()
    }
}

#[test]
fn fragmented_event_is_redacted_only_when_complete_and_drop_flushes() {
    register_secret("fragmented-private-secret");
    let capture = Capture::default();
    let make = RedactingMakeWriter::new(capture.clone());
    {
        let mut writer = make.make_writer();
        for part in [
            "frag",
            "mented-private-",
            "secret",
            " Bear",
            "er ",
            "other-secret",
        ] {
            writer.write_all(part.as_bytes()).unwrap();
        }
        assert!(capture.0.lock().unwrap().is_empty());
    }
    assert_eq!(&*capture.0.lock().unwrap(), b"[REDACTED] Bearer [REDACTED]");
}

#[test]
fn stdout_layer_uses_redacting_make_writer() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .without_time()
            .with_ansi(false)
            .with_writer(RedactingMakeWriter::new(capture.clone())),
    );
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("Authorization: Bearer stdout-private-secret");
    });
    let output = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(output.contains("[REDACTED]"));
    assert!(!output.contains("stdout-private-secret"));
}

#[test]
fn active_file_is_redacted_and_worker_flushes_on_guard_drop() {
    let directory = tempfile::tempdir().unwrap();
    register_secret("active-file-private-secret");
    let guard = init(directory.path()).unwrap();
    tracing::info!(
        "active-file-private-secret https://example.test/?X-Amz-Signature=private-signature"
    );
    drop(guard);
    let output = std::fs::read_to_string(directory.path().join("app.log")).unwrap();
    assert!(output.contains("[REDACTED]"));
    assert!(!output.contains("active-file-private-secret"));
    assert!(!output.contains("private-signature"));
}
