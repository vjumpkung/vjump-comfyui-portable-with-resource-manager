use super::*;
use crate::shared::process::{self, CommandSpec, DiscardSink, RunLimits};

#[tokio::test(start_paused = true)]
async fn relay_upload_idleness_does_not_close_a_long_download() {
    let (mut child, relay_child) = tokio::io::duplex(64);
    let (relay_server, mut server) = tokio::io::duplex(64);
    let upload = tokio::spawn(relay_upload(relay_child, relay_server));
    tokio::time::sleep(IDLE * 2).await;
    assert!(!upload.is_finished());
    child.write_all(b"late TLS request").await.unwrap();
    child.shutdown().await.unwrap();
    let mut bytes = Vec::new();
    server.read_to_end(&mut bytes).await.unwrap();
    upload.await.unwrap().unwrap();
    assert_eq!(bytes, b"late TLS request");
}

fn environment(values: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    values
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect()
}

async fn mock_upstream() -> (TcpListener, HfProxyConfig) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = format!(
        "http://user:secret-password@{}",
        listener.local_addr().unwrap()
    );
    let mut config =
        HfProxyConfig::from_environment(environment(&[("HTTPS_PROXY", &proxy)])).unwrap();
    // Test-only DNS seam: no public network is required and production private
    // address checks remain in force, even when chaining a proxy.
    config.resolved = Some(vec!["1.1.1.1:443".parse().unwrap()]);
    (listener, config)
}

async fn tunnel(relay: &HfRelay) -> TcpStream {
    let mut child = TcpStream::connect(relay.address).await.unwrap();
    child
        .write_all(b"CONNECT huggingface.co:443 HTTP/1.1\r\nHost: huggingface.co:443\r\n\r\n")
        .await
        .unwrap();
    assert!(
        read_header(&mut child)
            .await
            .unwrap()
            .starts_with(b"HTTP/1.1 200")
    );
    child
}

pub(crate) async fn mock_tunnel(limiter: Arc<BandwidthLimiter>) -> (HfRelay, TcpStream, TcpStream) {
    let (listener, config) = mock_upstream().await;
    let relay = HfRelay::start(limiter, config).await.unwrap();
    let upstream = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_header(&mut stream).await.unwrap();
        stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
        stream
    };
    let (source, child) = tokio::join!(upstream, tunnel(&relay));
    (relay, source, child)
}

#[tokio::test]
async fn relay_rejects_non_connect_and_invalid_authorities() {
    let relay = HfRelay::start(
        Arc::new(BandwidthLimiter::new(None)),
        HfProxyConfig::default(),
    )
    .await
    .unwrap();
    for request in [
        "GET https://huggingface.co/ HTTP/1.1\r\n\r\n",
        "CONNECT huggingface.co:80 HTTP/1.1\r\n\r\n",
        "CONNECT user@huggingface.co:443 HTTP/1.1\r\n\r\n",
        "CONNECT 127.0.0.1:443 HTTP/1.1\r\n\r\n",
        "CONNECT [::1]:443 HTTP/1.1\r\n\r\n",
        "CONNECT huggingface.co.:443 HTTP/1.1\r\n\r\n",
        "CONNECT huggingface.co:443/path HTTP/1.1\r\n\r\n",
        "CONNECT -bad.hf.co:443 HTTP/1.1\r\n\r\n",
        "CONNECT huggingface.co:443 HTTP/1.1\r\n folded: value\r\n\r\n",
    ] {
        let mut child = TcpStream::connect(relay.address).await.unwrap();
        child.write_all(request.as_bytes()).await.unwrap();
        assert!(
            read_header(&mut child)
                .await
                .unwrap()
                .starts_with(b"HTTP/1.1 502")
        );
    }
    let mut oversized = TcpStream::connect(relay.address).await.unwrap();
    oversized.write_all(&[b'A'; MAX_HEADER]).await.unwrap();
    assert!(
        read_header(&mut oversized)
            .await
            .unwrap()
            .starts_with(b"HTTP/1.1 502")
    );
    relay.shutdown().await;
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "192.168.1.1",
        "172.16.0.1",
        "169.254.1.1",
        "100.64.0.1",
        "0.0.0.0",
        "::1",
        "::ffff:127.0.0.1",
        "fc00::1",
        "fe80::1",
        "2001:db8::1",
    ] {
        assert!(!public_address(ip.parse().unwrap()), "{ip}");
    }
    let (listener, mut config) = mock_upstream().await;
    config.resolved = Some(vec!["127.0.0.1:443".parse().unwrap()]);
    assert!(connect("huggingface.co", &config).await.is_err());
    assert!(
        timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
}

#[test]
fn relay_host_checks_use_dns_label_boundaries() {
    for host in [
        "huggingface.co",
        "cdn.huggingface.co",
        "hf.co",
        "cas-bridge.xethub.hf.co",
        "HF.CO",
    ] {
        assert!(
            authority(
                format!("CONNECT {host}:443 HTTP/1.1\r\n\r\n").as_bytes(),
                RelayScope::HuggingFace
            )
            .is_ok(),
            "{host}"
        );
    }
    for host in [
        "evilhuggingface.co",
        "nothf.co",
        "hf.co.evil.com",
        "huggingface.co.evil",
        "hf..co",
        "evil/hf.co",
        "user@hf.co",
    ] {
        assert!(
            authority(
                format!("CONNECT {host}:443 HTTP/1.1\r\n\r\n").as_bytes(),
                RelayScope::HuggingFace
            )
            .is_err(),
            "{host}"
        );
    }
}

#[test]
fn relay_drive_scope_allows_only_google_hosts() {
    for host in [
        "drive.google.com",
        "docs.google.com",
        "drive.usercontent.google.com",
        "accounts.google.com",
        "doc-0s-8c-docs.googleusercontent.com",
    ] {
        assert!(RelayScope::GoogleDrive.allows(host), "{host}");
        assert!(
            authority(
                format!("CONNECT {host}:443 HTTP/1.1\r\n\r\n").as_bytes(),
                RelayScope::GoogleDrive
            )
            .is_ok(),
            "{host}"
        );
        assert!(!RelayScope::HuggingFace.allows(host), "{host}");
    }
    for host in [
        "huggingface.co",
        "evilgoogle.com",
        "google.com.evil",
        "notgoogleusercontent.com",
    ] {
        assert!(!RelayScope::GoogleDrive.allows(host), "{host}");
        assert!(
            authority(
                format!("CONNECT {host}:443 HTTP/1.1\r\n\r\n").as_bytes(),
                RelayScope::GoogleDrive
            )
            .is_err(),
            "{host}"
        );
    }
}

#[tokio::test]
async fn relay_chains_upstream_proxy_without_logging_credentials() {
    let (listener, config) = mock_upstream().await;
    let upstream = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let header = read_header(&mut stream).await.unwrap();
        // Assert without printing the credential-bearing request on failure.
        assert!(header.starts_with(b"CONNECT huggingface.co:443 HTTP/1.1\r\n"));
        assert!(
            header
                .windows(b"Proxy-Authorization: Basic ".len())
                .any(|w| w == b"Proxy-Authorization: Basic ")
        );
        let expected = STANDARD.encode("user:secret-password");
        assert!(
            header
                .windows(expected.len())
                .any(|w| w == expected.as_bytes())
        );
        stream
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\nopaque-server-tls")
            .await
            .unwrap();
        let mut tls = [0; 16];
        stream.read_exact(&mut tls).await.unwrap();
        assert_eq!(&tls, b"opaque-child-tls");
    });
    let relay = HfRelay::start(Arc::new(BandwidthLimiter::new(None)), config)
        .await
        .unwrap();
    let mut child = tunnel(&relay).await;
    child.write_all(b"opaque-child-tls").await.unwrap();
    let mut tls = [0; 17];
    child.read_exact(&mut tls).await.unwrap();
    assert_eq!(&tls, b"opaque-server-tls");
    upstream.await.unwrap();
    relay.shutdown().await;
    // Every surfaced error is a fixed code; it cannot contain a URL, header,
    // upstream password or raw io/TLS error. The relay has no logging calls.
    assert_eq!(ProxyUnavailable.to_string(), "bandwidth_proxy_unavailable");
    let bypass = HfProxyConfig::from_environment(environment(&[
        ("HTTPS_PROXY", "http://127.0.0.1:1"),
        ("NO_PROXY", ".hf.co,huggingface.co:443"),
    ]))
    .unwrap();
    assert!(bypass.bypasses("cdn.hf.co"));
    assert!(bypass.bypasses("huggingface.co"));
    assert!(!bypass.bypasses("evilhf.co"));
    for value in [
        "socks5://localhost:8080",
        "not a proxy",
        "http://localhost/path",
    ] {
        for key in [
            "https_proxy",
            "HTTPS_PROXY",
            "all_proxy",
            "ALL_PROXY",
            "http_proxy",
            "HTTP_PROXY",
        ] {
            assert!(HfProxyConfig::from_environment(environment(&[(key, value)])).is_err());
        }
    }
    for key in [
        "https_proxy",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
        "http_proxy",
        "HTTP_PROXY",
    ] {
        let config =
            HfProxyConfig::from_environment(environment(&[(key, "http://localhost:8080")]))
                .unwrap();
        assert_eq!(config.upstream.unwrap().port, 8080);
    }
    assert!(
        HfProxyConfig::from_environment(environment(&[("HTTPS_PROXY", "https://localhost:8443")]))
            .unwrap()
            .upstream
            .unwrap()
            .tls
            .is_some()
    );
}

#[tokio::test]
async fn hf_proxy_configuration_changes_only_child_environment() {
    let before: Vec<_> = std::env::vars_os().collect();
    let captured = HfProxyConfig::from_environment(environment(&[
        ("HTTPS_PROXY", "http://user:secret@localhost:1234"),
        ("NO_PROXY", "hf.co"),
    ]))
    .unwrap();
    let relay = HfRelay::start(Arc::new(BandwidthLimiter::new(None)), captured.clone())
        .await
        .unwrap();
    let mut child = environment(&[
        ("HTTPS_PROXY", "old"),
        ("NO_PROXY", "*"),
        ("all_proxy", "socks5://old"),
        ("FTP_PROXY", "old"),
        ("HF_TOKEN", "token"),
    ]);
    relay.configure_child(&mut child);
    for key in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
        let values: Vec<_> = child.iter().filter(|(k, _)| k == key).collect();
        assert_eq!(values.len(), 1);
        assert_eq!(
            values[0].1,
            OsString::from(format!("http://{}", relay.address))
        );
    }
    for key in ["NO_PROXY", "no_proxy"] {
        assert!(child.iter().any(|(k, v)| k == key && v.is_empty()));
    }
    for key in ["ALL_PROXY", "all_proxy", "FTP_PROXY"] {
        assert!(!child.iter().any(|(k, _)| k == key));
    }
    assert!(child.iter().any(|(k, v)| k == "HF_TOKEN" && v == "token"));
    assert!(std::env::vars_os().collect::<Vec<_>>() == before);
    assert!(captured.bypasses("hf.co"));
    assert_eq!(captured.upstream.unwrap().port, 1234);
    #[cfg(windows)]
    {
        let mixed = HfProxyConfig::from_environment(environment(&[
            ("Https_Proxy", "http://localhost:1234"),
            ("No_Proxy", "hf.co"),
        ]))
        .unwrap();
        assert_eq!(mixed.upstream.as_ref().unwrap().port, 1234);
        assert!(mixed.bypasses("hf.co"));
    }
    relay.shutdown().await;
}

#[tokio::test]
async fn hf_child_exit_closes_all_relay_connections() {
    for mode in ["success", "failure", "cancel", "spawn_error"] {
        let (listener, config) = mock_upstream().await;
        let upstream = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_header(&mut socket).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
            let mut byte = [0];
            // After relay shutdown the upstream must see EOF or a reset.
            matches!(
                timeout(Duration::from_secs(3), socket.read(&mut byte)).await,
                Ok(Ok(0) | Err(_))
            )
        });
        let relay = HfRelay::start(Arc::new(BandwidthLimiter::new(None)), config)
            .await
            .unwrap();
        let address = relay.address;
        let mut child_socket = tunnel(&relay).await;
        let mut pending_handshake = TcpStream::connect(address).await.unwrap();
        pending_handshake.write_all(b"CONNECT ").await.unwrap();
        #[cfg(windows)]
        let spec = if mode == "cancel" {
            CommandSpec::new(
                std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                    .join("System32/ping.exe"),
            )
            .args(["-n", "30", "127.0.0.1"])
        } else {
            CommandSpec::new(std::env::var_os("COMSPEC").unwrap()).args([
                "/C",
                if mode == "failure" {
                    "exit 1"
                } else {
                    "exit 0"
                },
            ])
        };
        #[cfg(unix)]
        let spec = if mode == "cancel" {
            CommandSpec::new("/bin/sleep").arg("30")
        } else {
            CommandSpec::new("/bin/sh").args([
                "-c",
                if mode == "failure" {
                    "exit 1"
                } else {
                    "exit 0"
                },
            ])
        };
        let mut spec = spec.env_vars(process::system_baseline());
        if mode == "spawn_error" {
            spec.program = std::env::current_dir()
                .unwrap()
                .join("missing-hf-executable");
        }
        relay.configure_child(&mut spec.env);
        let cancel = CancelToken::new();
        let limits = RunLimits::default();
        let run = process::run(&spec, &DiscardSink, &cancel, &limits);
        let (result, ()) = tokio::join!(run, async {
            if mode == "cancel" {
                tokio::time::sleep(Duration::from_millis(100)).await;
                cancel.cancel();
            }
        });
        match mode {
            "success" => assert!(result.is_ok()),
            "failure" => assert!(matches!(result, Err(process::RunError::Failed { .. }))),
            "cancel" => assert!(matches!(result, Err(process::RunError::Cancelled { .. }))),
            "spawn_error" => assert!(matches!(result, Err(process::RunError::Spawn(_)))),
            _ => unreachable!(),
        }
        relay.shutdown().await;
        assert!(upstream.await.unwrap());
        let mut byte = [0];
        assert!(matches!(
            timeout(Duration::from_secs(1), child_socket.read(&mut byte)).await,
            Ok(Ok(0) | Err(_))
        ));
        assert!(matches!(
            timeout(Duration::from_secs(1), pending_handshake.read(&mut byte)).await,
            Ok(Ok(0) | Err(_))
        ));
        assert!(TcpStream::connect(address).await.is_err());
    }
}

/// Opt-in acceptance check with the bundled hf CLI and hf-xet installed. Set
/// VJ_REAL_HF_EXE to its absolute path, VJ_REAL_HF_REPO, VJ_REAL_HF_REVISION to a
/// pinned commit, VJ_REAL_HF_FILE to a public, known Xet-backed file, and
/// VJ_REAL_HF_SIZE to its exact byte length (1-16 MiB). In a disposable sandbox,
/// deny the CLI/Python process tree all non-loopback egress, while allowing this
/// test executable outbound HTTPS (and any configured upstream proxy). Do not
/// disable Xet. The first, uncached CLI attempt must fail without a relay; the
/// second must download through the capped relay. The test does not change any
/// firewall rules. Run from the repository root:
/// `cargo test -p infrastructure --locked -j 3 real_hf_xet_obeys_relay_with_direct_egress_denied -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "requires real hf-xet, a public Xet file, network and externally denied child egress"]
async fn real_hf_xet_obeys_relay_with_direct_egress_denied() {
    let executable = std::path::PathBuf::from(std::env::var_os("VJ_REAL_HF_EXE").unwrap());
    assert!(executable.is_absolute() && executable.is_file());
    let repo = std::env::var("VJ_REAL_HF_REPO").unwrap();
    let revision = std::env::var("VJ_REAL_HF_REVISION").unwrap();
    assert!(revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()));
    let file = domain::shared::model_values::RelativeModelPath::new(
        &std::env::var("VJ_REAL_HF_FILE").unwrap(),
    )
    .unwrap();
    assert!(!file.components().is_empty());
    let size: u64 = std::env::var("VJ_REAL_HF_SIZE").unwrap().parse().unwrap();
    assert!((1024 * 1024..=16 * 1024 * 1024).contains(&size));
    let temp = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
    let command = |attempt: &str| {
        let root = temp.path().join(attempt);
        let mut env = process::system_baseline();
        env.extend([
            ("HF_HOME".into(), root.join("cache").into_os_string()),
            ("HF_HUB_DISABLE_TELEMETRY".into(), "1".into()),
            ("PYTHONNOUSERSITE".into(), "1".into()),
        ]);
        CommandSpec::new(&executable)
            .args([
                "download",
                &repo,
                &file.as_string(),
                "--revision",
                &revision,
                "--local-dir",
            ])
            .arg(root.join("files"))
            .arg("--force-download")
            .env_vars(env)
    };
    let cancel = CancelToken::new();
    let direct = process::run(
        &command("direct"),
        &DiscardSink,
        &cancel,
        &RunLimits {
            timeout: Some(Duration::from_secs(20)),
            ..RunLimits::default()
        },
    )
    .await;
    assert!(
        matches!(
            direct,
            Err(process::RunError::Failed { .. } | process::RunError::TimedOut { .. })
        ),
        "the uncached CLI must be denied direct egress"
    );
    let started = std::time::Instant::now();
    let relay = HfRelay::start(
        Arc::new(BandwidthLimiter::new(std::num::NonZeroU32::new(256))),
        HfProxyConfig::capture().unwrap(),
    )
    .await
    .unwrap();
    let mut spec = command("relay");
    relay.configure_child(&mut spec.env);
    let result = process::run(
        &spec,
        &DiscardSink,
        &cancel,
        &RunLimits {
            timeout: Some(Duration::from_secs(300)),
            ..RunLimits::default()
        },
    )
    .await;
    relay.shutdown().await;
    assert!(
        result.is_ok(),
        "hf-xet must succeed using only relay egress"
    );
    let downloaded = temp.path().join("relay/files").join(file.as_string());
    assert_eq!(std::fs::metadata(downloaded).unwrap().len(), size);
    assert!(size as f64 <= 256.0 * 1024.0 * started.elapsed().as_secs_f64() + MAX_HEADER as f64);
}
