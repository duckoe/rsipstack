use crate::sip::Port;
use crate::transport::tcp::TcpConnection;
use crate::transport::transport_layer::TransportLayerInnerRef;
use crate::transport::SipAddr;
use crate::transport::SipConnection;
use crate::Result;
use parking_lot::RwLock;
use std::fmt;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tracing::{debug, warn};
pub struct TcpListenerConnectionInner {
    pub local_addr: SipAddr,
    pub external: Option<SipAddr>,
    bound_port: RwLock<Option<Port>>,
}

#[derive(Clone)]
pub struct TcpListenerConnection {
    pub inner: Arc<TcpListenerConnectionInner>,
}

impl TcpListenerConnection {
    pub async fn new(local_addr: SipAddr, external: Option<SocketAddr>) -> Result<Self> {
        let inner = TcpListenerConnectionInner {
            bound_port: RwLock::new(local_addr.addr.port),
            local_addr,
            external: external.map(|addr| SipAddr {
                r#type: Some(crate::sip::transport::Transport::Tcp),
                addr: addr.into(),
            }),
        };
        Ok(TcpListenerConnection {
            inner: Arc::new(inner),
        })
    }

    pub async fn serve_listener(
        &self,
        transport_layer_inner: TransportLayerInnerRef,
    ) -> Result<()> {
        let listener = TcpListener::bind(self.inner.local_addr.get_socketaddr()?).await?;
        let listener_local_addr = SipAddr {
            r#type: Some(crate::sip::transport::Transport::Tcp),
            addr: listener.local_addr()?.into(),
        };
        // If specified port is 0, update bound port to ephemetal port chosen by OS
        if self.inner.local_addr.addr.port.is_some_and(|p| p.0 == 0) {
            self.inner
                .bound_port
                .write()
                // unwrap because we can assume the OS layer always returns a port value on a bound socket
                .replace(listener_local_addr.addr.port.unwrap());
        }
        tokio::spawn(async move {
            loop {
                let (stream, remote_addr) = match listener.accept().await {
                    Ok((stream, remote_addr)) => (stream, remote_addr),
                    Err(e) => {
                        warn!(error = ?e, "Failed to accept connection");
                        continue;
                    }
                };
                if !transport_layer_inner.is_whitelisted(remote_addr.ip()).await {
                    debug!(remote = %remote_addr, "tcp connection rejected by whitelist");
                    continue;
                }
                let local_addr = listener_local_addr.clone();
                let tcp_connection = match TcpConnection::from_stream(
                    stream,
                    local_addr.clone(),
                    Some(transport_layer_inner.cancel_token.child_token()),
                ) {
                    Ok(tcp_connection) => tcp_connection,
                    Err(e) => {
                        warn!(error = ?e, %local_addr, "Failed to create TCP connection");
                        continue;
                    }
                };
                let sip_connection = SipConnection::Tcp(tcp_connection.clone());
                transport_layer_inner.add_connection(sip_connection.clone());
                debug!(?local_addr, "new tcp connection");
            }
        });
        Ok(())
    }

    pub fn bound_port(&self) -> Option<Port> {
        *self.inner.bound_port.read()
    }
}

impl TcpListenerConnection {
    pub fn get_addr(&self) -> &SipAddr {
        if let Some(external) = &self.inner.external {
            external
        } else {
            &self.inner.local_addr
        }
    }

    pub async fn close(&self) -> Result<()> {
        Ok(())
    }
}

impl fmt::Display for TcpListenerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TCP Listener {}", self.get_addr())
    }
}

impl fmt::Debug for TcpListenerConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
