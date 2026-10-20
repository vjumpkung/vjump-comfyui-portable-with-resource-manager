//! Attempt-scoped CONNECT relay. Only the upstream TLS envelope is decrypted;
//! destination TLS and credentials remain opaque and are never logged.
use super::{bandwidth::BandwidthLimiter, native_engine::cancelled};
use application::shared::CancelToken;
use base64::{Engine, engine::general_purpose::STANDARD};
use percent_encoding::percent_decode_str;
use std::{
    ffi::OsString,
    io,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream, lookup_host},
    sync::oneshot,
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{self, pki_types::ServerName},
};
use url::Url;

const MAX_HEADER: usize = 16 * 1024;
const MAX_CONNECTIONS: usize = 64;
const HANDSHAKE: Duration = Duration::from_secs(15);
const IDLE: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("bandwidth_proxy_unavailable")]
pub struct ProxyUnavailable;

#[derive(Clone)]
struct Upstream {
    host: String,
    port: u16,
    tls: Option<TlsConnector>,
    authorization: Option<String>,
}

/// Captured before child overrides; unsupported configuration is retained as an
/// error for HF attempts, without preventing native downloads from starting.
#[derive(Clone, Default)]
pub struct HfProxyConfig {
    upstream: Option<Upstream>,
    bypass: Vec<String>,
    #[cfg(test)]
    resolved: Option<Vec<SocketAddr>>,
}
impl HfProxyConfig {
    pub fn capture() -> Result<Self, ProxyUnavailable> {
        Self::from_environment(std::env::vars_os())
    }

    fn from_environment(
        env: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Result<Self, ProxyUnavailable> {
        let values: Vec<_> = env.into_iter().collect();
        let get = |name: &str| -> Result<Option<String>, ProxyUnavailable> {
            values
                .iter()
                .find(|(k, _)| {
                    k == name || (cfg!(windows) && k.to_string_lossy().eq_ignore_ascii_case(name))
                })
                .map(|(_, v)| v.to_str().map(str::to_owned).ok_or(ProxyUnavailable))
                .transpose()
                .map(|v| v.filter(|s| !s.is_empty()))
        };
        let proxy = get("https_proxy")?
            .or(get("HTTPS_PROXY")?)
            .or(get("all_proxy")?)
            .or(get("ALL_PROXY")?)
            .or(get("http_proxy")?)
            .or(get("HTTP_PROXY")?);
        let bypass = get("no_proxy")?.or(get("NO_PROXY")?).unwrap_or_default();
        let mut config = Self::default();
        for entry in bypass.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if entry != "*"
                && (!entry.is_ascii()
                    || entry.contains(['/', '@', '?', '#'])
                    || entry.chars().any(char::is_whitespace))
            {
                return Err(ProxyUnavailable);
            }
            config.bypass.push(entry.to_ascii_lowercase());
        }
        if let Some(proxy) = proxy {
            let url = Url::parse(&proxy).map_err(|_| ProxyUnavailable)?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !matches!(url.path(), "" | "/")
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(ProxyUnavailable);
            }
            let authorization = if !url.username().is_empty() || url.password().is_some() {
                let user = percent_decode_str(url.username())
                    .decode_utf8()
                    .map_err(|_| ProxyUnavailable)?;
                let password = percent_decode_str(url.password().unwrap_or(""))
                    .decode_utf8()
                    .map_err(|_| ProxyUnavailable)?;
                Some(format!(
                    "Basic {}",
                    STANDARD.encode(format!("{user}:{password}"))
                ))
            } else {
                None
            };
            let tls = if url.scheme() == "https" {
                let mut roots = rustls::RootCertStore::empty();
                let loaded = rustls_native_certs::load_native_certs();
                roots.add_parsable_certificates(loaded.certs);
                if roots.is_empty() {
                    return Err(ProxyUnavailable);
                }
                let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
                let client = rustls::ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .map_err(|_| ProxyUnavailable)?
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                Some(TlsConnector::from(Arc::new(client)))
            } else {
                None
            };
            config.upstream = Some(Upstream {
                host: url
                    .host_str()
                    .ok_or(ProxyUnavailable)?
                    .trim_matches(['[', ']'])
                    .to_owned(),
                port: url.port_or_known_default().ok_or(ProxyUnavailable)?,
                tls,
                authorization,
            });
        }
        Ok(config)
    }

    fn bypasses(&self, host: &str) -> bool {
        self.bypass.iter().any(|entry| {
            if entry == "*" {
                return true;
            }
            let (domain, port) = entry
                .split_once(':')
                .map_or((entry.as_str(), None), |(d, p)| (d, Some(p)));
            if port.is_some_and(|p| p != "443") {
                return false;
            }
            let domain = domain.trim_start_matches('.');
            host == domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
    }
}

/// The destination hosts a relay accepts for one tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayScope {
    HuggingFace,
    GoogleDrive,
}
impl RelayScope {
    fn allows(self, host: &str) -> bool {
        match self {
            Self::HuggingFace => host_allowed(host),
            Self::GoogleDrive => {
                host == "google.com"
                    || host.ends_with(".google.com")
                    || host.ends_with(".googleusercontent.com")
            }
        }
    }
}

pub struct HfRelay {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    failed: Arc<AtomicBool>,
}
impl HfRelay {
    pub async fn start(
        limiter: Arc<BandwidthLimiter>,
        config: HfProxyConfig,
    ) -> Result<Self, ProxyUnavailable> {
        Self::start_scoped(limiter, config, RelayScope::HuggingFace).await
    }

    pub async fn start_scoped(
        limiter: Arc<BandwidthLimiter>,
        config: HfProxyConfig,
        scope: RelayScope,
    ) -> Result<Self, ProxyUnavailable> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| ProxyUnavailable)?;
        let address = listener.local_addr().map_err(|_| ProxyUnavailable)?;
        let (stop, mut stopped) = oneshot::channel();
        let failed = Arc::new(AtomicBool::new(false));
        let failures = failed.clone();
        let config = Arc::new(config);
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    _ = connections.join_next(), if !connections.is_empty() => {},
                    accepted = listener.accept() => {
                        let Ok((child, _)) = accepted else { failures.store(true, Ordering::SeqCst); break; };
                        if connections.len() >= MAX_CONNECTIONS { drop(child); continue; }
                        let limiter = limiter.clone();
                        let config = config.clone();
                        let failures = failures.clone();
                        connections.spawn(async move {
                            if serve(child, limiter, &config, scope).await.is_err() {
                                failures.store(true, Ordering::SeqCst);
                            }
                        });
                    }
                }
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        Ok(Self {
            address,
            stop: Some(stop),
            task: Some(task),
            failed,
        })
    }

    pub fn configure_child(&self, env: &mut Vec<(OsString, OsString)>) {
        // process::run uses env_clear. Remove all proxy spellings defensively,
        // then provide exactly these values; no bypass reaches Python or Xet.
        env.retain(|(key, _)| {
            !matches!(
                key.to_string_lossy().to_ascii_uppercase().as_str(),
                "HTTP_PROXY"
                    | "HTTPS_PROXY"
                    | "ALL_PROXY"
                    | "NO_PROXY"
                    | "FTP_PROXY"
                    | "RSYNC_PROXY"
            )
        });
        let proxy = format!("http://{}", self.address);
        for key in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
            env.push((key.into(), proxy.clone().into()));
        }
        for key in ["NO_PROXY", "no_proxy"] {
            env.push((key.into(), "".into()));
        }
    }

    pub async fn shutdown(mut self) -> bool {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        self.failed.load(Ordering::SeqCst)
    }
}
impl Drop for HfRelay {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn host_allowed(host: &str) -> bool {
    host == "huggingface.co"
        || host.ends_with(".huggingface.co")
        || host == "hf.co"
        || host.ends_with(".hf.co")
}

fn authority(request: &[u8], scope: RelayScope) -> Result<String, ProxyUnavailable> {
    let text = std::str::from_utf8(request).map_err(|_| ProxyUnavailable)?;
    let line = text.split("\r\n").next().ok_or(ProxyUnavailable)?;
    let parts: Vec<_> = line.split(' ').collect();
    if parts.len() != 3 || parts[0] != "CONNECT" || !matches!(parts[2], "HTTP/1.1" | "HTTP/1.0") {
        return Err(ProxyUnavailable);
    }
    let host = parts[1]
        .strip_suffix(":443")
        .ok_or(ProxyUnavailable)?
        .to_ascii_lowercase();
    if host.len() > 253
        || host.parse::<IpAddr>().is_ok()
        || !scope.allows(&host)
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(ProxyUnavailable);
    }
    // Reject malformed headers, folding, and control characters. Never log them.
    for line in text
        .strip_suffix("\r\n\r\n")
        .ok_or(ProxyUnavailable)?
        .split("\r\n")
        .skip(1)
    {
        let (key, value) = line.split_once(':').ok_or(ProxyUnavailable)?;
        if key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.bytes().any(|b| b.is_ascii_control() && b != b'\t')
        {
            return Err(ProxyUnavailable);
        }
    }
    Ok(host)
}

fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_broadcast()
                && a != 0
                && a < 240
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 198 && (18..=19).contains(&b))
                && !ip.is_documentation()
        }
        IpAddr::V6(ip) => {
            // Only global unicast; reject mapped/compatible IPv4, ULA/link local,
            // documentation and transition prefixes that can hide private IPv4.
            let segments = ip.segments();
            segments[0] & 0xe000 == 0x2000
                && segments[0] != 0x2002
                && !(segments[0] == 0x2001 && (segments[1] < 0x200 || segments[1] == 0xdb8))
        }
    }
}

async fn read_header(
    stream: &mut (impl AsyncRead + Unpin + ?Sized),
) -> Result<Vec<u8>, ProxyUnavailable> {
    let mut header = Vec::new();
    // Single-byte reads avoid consuming destination TLS bytes after the header.
    while header.len() < MAX_HEADER {
        header.push(stream.read_u8().await.map_err(|_| ProxyUnavailable)?);
        if header.ends_with(b"\r\n\r\n") {
            return Ok(header);
        }
    }
    Err(ProxyUnavailable)
}

trait Tunnel: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Tunnel for T {}

async fn connect(host: &str, config: &HfProxyConfig) -> Result<Box<dyn Tunnel>, ProxyUnavailable> {
    #[cfg(test)]
    let addresses = if let Some(addresses) = &config.resolved {
        addresses.clone()
    } else {
        lookup_host((host, 443))
            .await
            .map_err(|_| ProxyUnavailable)?
            .collect::<Vec<_>>()
    };
    #[cfg(not(test))]
    let addresses: Vec<_> = lookup_host((host, 443))
        .await
        .map_err(|_| ProxyUnavailable)?
        .collect();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| !public_address(address.ip()))
    {
        return Err(ProxyUnavailable);
    }
    if let Some(upstream) = config.upstream.as_ref().filter(|_| !config.bypasses(host)) {
        let tcp = TcpStream::connect((upstream.host.as_str(), upstream.port))
            .await
            .map_err(|_| ProxyUnavailable)?;
        let mut stream: Box<dyn Tunnel> = if let Some(tls) = &upstream.tls {
            let name = ServerName::try_from(upstream.host.clone()).map_err(|_| ProxyUnavailable)?;
            Box::new(tls.connect(name, tcp).await.map_err(|_| ProxyUnavailable)?)
        } else {
            Box::new(tcp)
        };
        let mut request = format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n");
        if let Some(authorization) = &upstream.authorization {
            request.push_str("Proxy-Authorization: ");
            request.push_str(authorization);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|_| ProxyUnavailable)?;
        let header = read_header(&mut *stream).await?;
        let status = std::str::from_utf8(&header)
            .map_err(|_| ProxyUnavailable)?
            .split("\r\n")
            .next()
            .ok_or(ProxyUnavailable)?;
        let mut parts = status.split(' ');
        if !matches!(parts.next(), Some("HTTP/1.0" | "HTTP/1.1")) || parts.next() != Some("200") {
            return Err(ProxyUnavailable);
        }
        Ok(stream)
    } else {
        // Connect to the checked address, never resolve again (DNS rebinding).
        for address in addresses {
            if let Ok(stream) = TcpStream::connect(address).await {
                return Ok(Box::new(stream));
            }
        }
        Err(ProxyUnavailable)
    }
}

async fn serve(
    mut child: TcpStream,
    limiter: Arc<BandwidthLimiter>,
    config: &HfProxyConfig,
    scope: RelayScope,
) -> Result<(), ProxyUnavailable> {
    let handshake = async {
        let header = read_header(&mut child).await?;
        let host = authority(&header, scope)?;
        connect(&host, config).await
    };
    let upstream = match timeout(HANDSHAKE, handshake).await {
        Ok(Ok(stream)) => stream,
        _ => {
            let _ = timeout(
                HANDSHAKE,
                child.write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n"),
            )
            .await;
            return Err(ProxyUnavailable);
        }
    };
    timeout(
        HANDSHAKE,
        child.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n"),
    )
    .await
    .map_err(|_| ProxyUnavailable)?
    .map_err(|_| ProxyUnavailable)?;
    let (child_read, child_write) = child.into_split();
    let (server_read, server_write) = tokio::io::split(upstream);
    let cancel = CancelToken::new();
    tokio::try_join!(
        relay_upload(child_read, server_write),
        relay_download(server_read, child_write, &limiter, &cancel)
    )
    .map_err(|_| ProxyUnavailable)?;
    Ok(())
}

async fn relay_upload(
    mut child: impl AsyncRead + Unpin,
    mut server: impl AsyncWrite + Unpin,
) -> io::Result<()> {
    let mut buffer = [0; 16 * 1024];
    loop {
        // A long, capped download need not send anything upstream. The
        // child lifetime owns this read; upload idleness is not a failure.
        let n = child.read(&mut buffer).await?;
        if n == 0 {
            return server.shutdown().await;
        }
        timeout(IDLE, server.write_all(&buffer[..n]))
            .await
            .map_err(io::Error::other)??;
    }
}

pub(super) async fn relay_download(
    mut server: impl AsyncRead + Unpin,
    mut child: impl AsyncWrite + Unpin,
    limiter: &BandwidthLimiter,
    cancel: &CancelToken,
) -> io::Result<()> {
    let mut buffer = [0; 16 * 1024];
    loop {
        let n = tokio::select! {
            _ = cancelled(cancel) => return Err(io::ErrorKind::Interrupted.into()),
            n = timeout(IDLE, server.read(&mut buffer)) => n.map_err(io::Error::other)??,
        };
        if n == 0 {
            return child.shutdown().await;
        }
        let mut offset = 0;
        while offset < n {
            let grant = limiter
                .acquire(n - offset, cancel)
                .await
                .map_err(io::Error::other)?;
            tokio::select! {
                _ = cancelled(cancel) => return Err(io::ErrorKind::Interrupted.into()),
                result = timeout(IDLE, child.write_all(&buffer[offset..offset + grant])) => result.map_err(io::Error::other)??,
            }
            offset += grant;
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
