//! A tiny HTTP/1.1 fixture server for tests. It serves scripted routes on `127.0.0.1`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Clone)]
pub enum Route {
    /// A complete response.
    Body {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// Announces `declared` bytes, sends `body`, then closes the connection.
    Truncated { body: Vec<u8>, declared: usize },
    /// A redirect to `location`.
    Redirect(String),
}

impl Route {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self::Body {
            status: 200,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self::Body {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }
}

pub struct TestServer {
    pub base_url: String,
    routes: Arc<Mutex<HashMap<String, Route>>>,
    pub requests: Arc<Mutex<Vec<String>>>,
}

impl TestServer {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let routes: Arc<Mutex<HashMap<String, Route>>> = Arc::default();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let task_routes = routes.clone();
        let task_requests = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let routes = task_routes.clone();
                let requests = task_requests.clone();
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 8192];
                    let mut read = 0;
                    loop {
                        let Ok(count) = socket.read(&mut buffer[read..]).await else {
                            return;
                        };
                        if count == 0 {
                            return;
                        }
                        read += count;
                        if buffer[..read]
                            .windows(4)
                            .any(|window| window == b"\r\n\r\n")
                        {
                            break;
                        }
                    }
                    let head = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    let path = head
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_owned();
                    requests.lock().unwrap().push(head);
                    let route = routes.lock().unwrap().get(&path).cloned();
                    let response = match route {
                        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                        Some(Route::Redirect(location)) => format!(
                            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                        .into_bytes(),
                        Some(Route::Body { status, headers, body }) => {
                            let mut text = format!(
                                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                                body.len()
                            );
                            for (name, value) in headers {
                                text.push_str(&format!("{name}: {value}\r\n"));
                            }
                            text.push_str("\r\n");
                            let mut bytes = text.into_bytes();
                            bytes.extend(body);
                            bytes
                        }
                        Some(Route::Truncated { body, declared }) => {
                            let mut bytes = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
                            )
                            .into_bytes();
                            bytes.extend(body);
                            bytes
                        }
                    };
                    let _ = socket.write_all(&response).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            routes,
            requests,
        }
    }

    pub fn route(&self, path: &str, route: Route) {
        self.routes.lock().unwrap().insert(path.to_owned(), route);
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}
