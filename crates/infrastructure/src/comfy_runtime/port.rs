use application::comfy_runtime::{PortProbe, RuntimeError};
use async_trait::async_trait;
use domain::comfy_runtime::Port;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};

pub struct TcpPortProbe;
#[async_trait]
impl PortProbe for TcpPortProbe {
    async fn available(&self, port: Port, listen_lan: bool) -> Result<bool, RuntimeError> {
        available(port, listen_lan)
    }
    async fn next_free(&self, after: Port, listen_lan: bool) -> Result<Option<Port>, RuntimeError> {
        for number in u32::from(after.get()) + 1..=u32::from(u16::MAX) {
            let port = Port::new(number as u16)?;
            if available(port, listen_lan)? {
                return Ok(Some(port));
            }
        }
        Ok(None)
    }
}
fn available(port: Port, lan: bool) -> Result<bool, RuntimeError> {
    let address = SocketAddrV4::new(
        if lan {
            Ipv4Addr::UNSPECIFIED
        } else {
            Ipv4Addr::LOCALHOST
        },
        port.get(),
    );
    match TcpListener::bind(address) {
        Ok(_) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            Ok(false)
        }
        Err(_) => Err(RuntimeError::Failed(
            "the requested bind address could not be checked".into(),
        )),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn detects_conflicts_without_wrapping() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = Port::new(listener.local_addr().unwrap().port()).unwrap();
        assert!(!TcpPortProbe.available(port, false).await.unwrap());
        assert_eq!(
            TcpPortProbe
                .next_free(Port::new(65535).unwrap(), false)
                .await
                .unwrap(),
            None
        );
    }
}
