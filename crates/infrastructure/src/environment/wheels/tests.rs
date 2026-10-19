use std::sync::Arc;

use application::{
    environment::{NullSink, RunId, StepId},
    shared::CancelToken,
};
use domain::shared::model_values::Sha256;

use super::*;
use crate::shared::{
    download::sha256_hex,
    test_http::{Route, TestServer},
};

fn context() -> StepContext {
    StepContext {
        run_id: RunId::new("wheel-test").unwrap(),
        step: StepId::Accelerators,
        sink: Arc::new(NullSink),
        cancel: CancelToken::new(),
        allow_source_builds: false,
        retained: Default::default(),
    }
}

fn artifact(url: String, bytes: &[u8]) -> AcceleratorSpec {
    AcceleratorSpec {
        id: "fixture".into(),
        name: "Fixture wheel".into(),
        modules: vec![],
        source: AcceleratorSource::Wheel {
            url,
            filename: SafeFileName::new("fixture-1-py3-none-any.whl").unwrap(),
            sha256: Sha256::new(&sha256_hex(bytes)).unwrap(),
            size: Some(bytes.len() as u64),
        },
        no_deps: false,
        default_selected: false,
        non_fatal: true,
        provenance: "fixture".into(),
    }
}

#[tokio::test]
async fn wheel_cache_is_rehashed_before_reuse() {
    let server = TestServer::start().await;
    let bytes = b"verified wheel bytes";
    server.route("/wheel", Route::ok(bytes));
    let root = tempfile::tempdir().unwrap();
    let wheels = Wheels {
        downloader: Downloader::with_loopback_http(),
    };
    let spec = artifact(server.url("/wheel"), bytes);
    let path = wheels
        .fetch_artifact(root.path(), &spec, &context())
        .await
        .unwrap();
    assert!(path.ends_with(format!("{}/fixture-1-py3-none-any.whl", sha256_hex(bytes))));
    wheels
        .fetch_artifact(root.path(), &spec, &context())
        .await
        .unwrap();
    assert_eq!(
        server.requests.lock().unwrap().len(),
        1,
        "valid cache is reused"
    );
    fs::write(&path, b"tampered wheel bytes").unwrap();
    server.route("/wheel", Route::ok(b"incorrect download"));
    assert!(
        wheels
            .fetch_artifact(root.path(), &spec, &context())
            .await
            .is_err()
    );
    assert!(
        !path.exists(),
        "modified cache cannot be handed to the installer"
    );
    server.route("/wheel", Route::ok(bytes));
    assert_eq!(
        fs::read(
            wheels
                .fetch_artifact(root.path(), &spec, &context())
                .await
                .unwrap()
        )
        .unwrap(),
        bytes
    );
}

#[tokio::test]
async fn wheel_fetch_enforces_size_cancellation_and_https() {
    let server = TestServer::start().await;
    server.route("/large", Route::ok(b"too large"));
    let root = tempfile::tempdir().unwrap();
    let wheels = Wheels {
        downloader: Downloader::with_loopback_http(),
    };
    let spec = artifact(server.url("/large"), b"tiny");
    assert!(
        matches!(wheels.fetch_artifact(root.path(), &spec, &context()).await, Err(ProvisionError::Failed(message)) if message.contains("larger"))
    );
    assert!(
        wheels.fetch(root.path(), &spec, &context()).await.is_err(),
        "public port still refuses HTTP"
    );
    let ctx = context();
    ctx.cancel.cancel();
    assert_eq!(
        wheels.fetch_artifact(root.path(), &spec, &ctx).await,
        Err(ProvisionError::Cancelled)
    );
    let mut unknown_size = spec;
    if let AcceleratorSource::Wheel { size, .. } = &mut unknown_size.source {
        *size = None;
    }
    server.route(
        "/large",
        Route::Truncated {
            body: Vec::new(),
            declared: (2usize << 30) + 1,
        },
    );
    assert!(
        matches!(wheels.fetch_artifact(root.path(), &unknown_size, &context()).await,
        Err(ProvisionError::Failed(message)) if message.contains("larger")),
        "unknown-size wheels have a 2 GiB cap"
    );
}
