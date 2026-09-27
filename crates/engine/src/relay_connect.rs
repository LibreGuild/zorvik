//! Raw byte streams to a TCP server, optionally with TLS, for callers that
//! move bytes themselves (the TCP relay). Direct connections only: no proxy.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::error::Result;
use crate::http::Client;
use crate::net::{self, Target};
use crate::tls::{Alpn, TlsInfo, TlsOptions};

/// A connected TCP stream (TLS already negotiated when asked for).
pub struct RawStream(net::Stream);

pub struct RawConnection {
    pub stream: RawStream,
    pub remote_addr: SocketAddr,
    pub local_addr: Option<SocketAddr>,
    /// Set for TLS connections.
    pub tls: Option<TlsInfo>,
}

impl Client {
    /// Connect to `host:port` (DNS, Happy Eyeballs, then TLS without ALPN when
    /// `tls` is set, verified per `tls_options`), all within `timeout`.
    pub async fn connect_raw(
        &self,
        host: &str,
        port: u16,
        tls: bool,
        tls_options: &TlsOptions,
        timeout: Duration,
    ) -> Result<RawConnection> {
        let conn = net::connect(
            &Target {
                host,
                port,
                tls,
                alpn: Alpn::None,
                tls_options,
                proxy: None,
                force_tunnel: false,
                connect_timeout: timeout,
            },
            &self.tls,
        )
        .await?;
        let local_addr = match &conn.stream {
            net::Stream::Plain(s) => s.local_addr().ok(),
            net::Stream::Tls(s) => s.get_ref().0.local_addr().ok(),
        };
        Ok(RawConnection { stream: RawStream(conn.stream), remote_addr: conn.remote_addr, local_addr, tls: conn.tls })
    }
}

impl AsyncRead for RawStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl AsyncWrite for RawStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }
}
