//! Bounded loopback readiness requests.
use application::comfy_runtime::RuntimeError;
use domain::comfy_runtime::Port;
use std::time::Duration;

pub struct SystemStatsProbe {
    client: reqwest::Client,
}
impl SystemStatsProbe {
    pub fn new() -> Result<Self, RuntimeError> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_millis(500))
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|_| {
                RuntimeError::Failed(
                    "the loopback readiness client could not be initialized".into(),
                )
            })?;
        Ok(Self { client })
    }
    /// HTTP is readiness evidence only. The supervisor separately verifies process ownership.
    pub async fn ready(&self, port: Port) -> bool {
        let Ok(mut response) = self
            .client
            .get(format!("http://127.0.0.1:{}/system_stats", port.get()))
            .send()
            .await
        else {
            return false;
        };
        if !response.status().is_success() || response.content_length().is_some_and(|n| n > 1 << 20)
        {
            return false;
        }
        let mut bytes = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if bytes.len() + chunk.len() <= 1 << 20 => {
                    bytes.extend_from_slice(&chunk)
                }
                Ok(None) => break,
                _ => return false,
            }
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return false;
        };
        value
            .get("system")
            .is_some_and(serde_json::Value::is_object)
            && value
                .get("devices")
                .is_some_and(serde_json::Value::is_array)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    async fn check(status: &str, body: &str) -> bool {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request).await;
            let _ = socket.write_all(reply.as_bytes()).await;
        });
        let ready = SystemStatsProbe::new()
            .unwrap()
            .ready(Port::new(port).unwrap())
            .await;
        server.await.unwrap();
        ready
    }
    #[tokio::test]
    async fn requires_successful_bounded_comfy_json() {
        assert!(check("200 OK", r#"{"system":{},"devices":[]}"#).await);
        assert!(!check("302 Found", r#"{"system":{},"devices":[]}"#).await);
        assert!(!check("200 OK", "<html>hello</html>").await);
        assert!(!check("200 OK", &" ".repeat((1 << 20) + 1)).await);
        assert!(!check("200 OK", "{}").await);
    }
}
