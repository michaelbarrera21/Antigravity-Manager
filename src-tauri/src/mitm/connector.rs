use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::Uri;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

/// TCP connector that routes through a SOCKS5 proxy.
/// Used as the inner connector for hyper-rustls HttpsConnector.
#[derive(Clone)]
pub struct Socks5Connector {
    proxy_addr: String,
}

impl Socks5Connector {
    pub fn new(socks5_url: &str) -> Self {
        let addr = socks5_url
            .strip_prefix("socks5://")
            .or_else(|| socks5_url.strip_prefix("socks5h://"))
            .unwrap_or(socks5_url);
        Self {
            proxy_addr: addr.to_string(),
        }
    }
}

impl tower::Service<Uri> for Socks5Connector {
    type Response = TokioIo<TcpStream>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let proxy_addr = self.proxy_addr.clone();

        Box::pin(async move {
            let host = uri.host().ok_or("no host in URI")?;
            let port = uri.port_u16().unwrap_or(
                if uri.scheme_str() == Some("https") { 443 } else { 80 },
            );
            let target = format!("{}:{}", host, port);

            let stream = tokio_socks::tcp::Socks5Stream::connect(
                proxy_addr.as_str(),
                target.as_str(),
            )
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("SOCKS5 connect to {} via {}: {}", target, proxy_addr, e).into()
            })?;

            Ok(TokioIo::new(stream.into_inner()))
        })
    }
}
