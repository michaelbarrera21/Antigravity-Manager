use crate::mitm::config::MitmConfig;
use crate::mitm::monitor::MitmMonitor;
use crate::mitm::resolver::DomainResolver;
use http::{Request, Response};
use hudsucker::{Body, HttpContext, HttpHandler, RequestOrResponse};
use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};

const BODY_COLLECT_TIMEOUT_SECS: u64 = 10;
const BODY_MAX_SIZE: usize = 10 * 1024 * 1024; // 10 MB

fn extract_connect_host(uri: &http::Uri) -> String {
    if let Some(host) = uri.host() {
        return host.to_string();
    }
    if let Some(auth) = uri.authority() {
        return auth.host().to_string();
    }
    let s = uri.to_string();
    s.split(':').next().unwrap_or("").to_string()
}

fn extract_request_host(req: &Request<Body>) -> String {
    if let Some(host) = req.headers().get("host").and_then(|v| v.to_str().ok()) {
        return host.split(':').next().unwrap_or(host).to_string();
    }
    if let Some(host) = req.uri().host() {
        return host.to_string();
    }
    String::new()
}

fn rewrite_uri_with_host(req: Request<Body>, host: &str) -> Request<Body> {
    let old_uri = req.uri().clone();
    let uri_host = old_uri.host().unwrap_or("");

    if uri_host == host || host.is_empty() {
        return req;
    }

    let scheme = old_uri.scheme_str().unwrap_or("https");
    let path = old_uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let new_uri_str = format!("{}://{}{}", scheme, host, path);

    match new_uri_str.parse::<http::Uri>() {
        Ok(new_uri) => {
            tracing::debug!("[MITM] URI 重写: {} → {}", old_uri, new_uri);
            let (mut parts, body) = req.into_parts();
            parts.uri = new_uri;
            Request::from_parts(parts, body)
        }
        Err(e) => {
            tracing::warn!("[MITM] URI 重写失败: {}", e);
            req
        }
    }
}

fn is_streaming_url(url: &str) -> bool {
    url.contains("streamGenerateContent")
        || url.contains("stream=true")
        || url.contains("alt=sse")
}

#[derive(Clone)]
pub struct AntigravityHttpHandler {
    pub monitor: Arc<MitmMonitor>,
    pub config: MitmConfig,
    pub requests_processed: Arc<AtomicUsize>,
    pub resolver: Arc<DomainResolver>,
    pub req_uri: Option<String>,
    pub req_method: Option<String>,
    pub req_headers: std::collections::HashMap<String, String>,
    pub req_body_str: Option<String>,
    pub req_start: Option<std::time::Instant>,
    is_target: bool,
}

impl AntigravityHttpHandler {
    pub fn new(
        monitor: Arc<MitmMonitor>,
        config: MitmConfig,
        requests_processed: Arc<AtomicUsize>,
        resolver: Arc<DomainResolver>,
    ) -> Self {
        Self {
            monitor,
            config,
            requests_processed,
            resolver,
            req_uri: None,
            req_method: None,
            req_headers: std::collections::HashMap::new(),
            req_body_str: None,
            req_start: None,
            is_target: false,
        }
    }
}

impl AntigravityHttpHandler {
    /// SSE / streaming response: tee the body through a channel so the client
    /// receives data in real-time while we buffer a copy for logging.
    /// Logging happens asynchronously when the stream finishes.
    fn handle_streaming_response(
        &mut self,
        res: Response<Body>,
        method: String,
        uri: String,
        response_headers: std::collections::HashMap<String, String>,
        status: u16,
        req_start: Option<std::time::Instant>,
    ) -> Response<Body> {
        use futures::StreamExt;
        use http_body_util::BodyStream;

        let (parts, body) = res.into_parts();
        let mut body_stream = BodyStream::new(body);

        let (tx, rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(64);

        let monitor = self.monitor.clone();
        let req_headers = self.req_headers.clone();
        let req_body = self.req_body_str.clone();

        tracing::info!("[MITM] ← {} {} [{}] SSE stream start", method, uri, status);

        tokio::spawn(async move {
            let mut buffer = Vec::new();
            while let Some(frame_result) = body_stream.next().await {
                match frame_result {
                    Ok(frame) => {
                        if let Some(data) = frame.data_ref() {
                            if buffer.len() < BODY_MAX_SIZE {
                                let remaining = BODY_MAX_SIZE - buffer.len();
                                let to_copy = data.len().min(remaining);
                                buffer.extend_from_slice(&data[..to_copy]);
                            }
                            if tx.send(data.clone()).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[MITM] SSE stream error {}: {}", uri, e);
                        break;
                    }
                }
            }
            drop(tx);

            let duration_ms = req_start.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
            let body_str = String::from_utf8_lossy(&buffer).to_string();
            tracing::info!(
                "[MITM] ← {} {} [{}] SSE stream done, {} bytes, {}ms",
                method, uri, status, buffer.len(), duration_ms
            );

            monitor.log_request(
                &method, &uri,
                req_headers, response_headers,
                req_body.as_deref(), Some(&body_str),
                duration_ms, status,
            ).await;
        });

        let rx_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        let frame_stream = rx_stream.map(|chunk| {
            Ok::<hyper::body::Frame<bytes::Bytes>, hudsucker::Error>(
                hyper::body::Frame::data(chunk),
            )
        });
        let new_body = http_body_util::StreamBody::new(frame_stream);
        Response::from_parts(parts, Body::from(new_body))
    }
}

impl HttpHandler for AntigravityHttpHandler {
    async fn should_intercept(
        &mut self,
        _ctx: &HttpContext,
        req: &Request<Body>,
    ) -> bool {
        let host = extract_connect_host(req.uri());

        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            if self.resolver.is_target_ip(&ip) {
                let domain = self.resolver.lookup_domain(&ip).unwrap_or_default();
                tracing::info!("[MITM] CONNECT → {} (目标域名: {}), 拦截", host, domain);
                return true;
            }
            tracing::debug!("[MITM] CONNECT → {} (非目标 IP), 隧道透传", host);
            return false;
        }

        let should = self.config.is_target_domain(&host);
        if should {
            tracing::info!("[MITM] CONNECT → {} (目标域名), 拦截", host);
        } else {
            tracing::debug!("[MITM] CONNECT → {} (非目标域名), 隧道透传", host);
        }
        should
    }

    async fn handle_error(
        &mut self,
        _ctx: &HttpContext,
        err: hyper_util::client::legacy::Error,
    ) -> Response<Body> {
        if !self.is_target {
            return Response::builder()
                .status(http::StatusCode::BAD_GATEWAY)
                .body(Body::from(format!("proxy error: {}", err)))
                .unwrap_or_else(|_| Response::new(Body::from("proxy error")));
        }

        let uri = self.req_uri.clone().unwrap_or_default();
        let method = self.req_method.clone().unwrap_or_else(|| "*".to_string());
        let duration_ms = self.req_start.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
        tracing::error!("[MITM] 代理请求失败 {} {}: {}", method, uri, err);

        self.monitor.log_request(
            &method,
            &uri,
            self.req_headers.clone(),
            std::collections::HashMap::new(),
            self.req_body_str.as_deref(),
            Some(&format!("MITM proxy error: {}", err)),
            duration_ms,
            502,
        ).await;

        Response::builder()
            .status(http::StatusCode::BAD_GATEWAY)
            .body(Body::from(format!("MITM proxy error: {}", err)))
            .unwrap_or_else(|_| {
                Response::new(Body::from("MITM proxy error"))
            })
    }

    async fn handle_request(
        &mut self,
        _ctx: &HttpContext,
        req: Request<Body>,
    ) -> RequestOrResponse {
        if req.method() == http::Method::CONNECT {
            return RequestOrResponse::Request(req);
        }

        let host = extract_request_host(&req);
        let req = rewrite_uri_with_host(req, &host);

        self.is_target = self.config.is_target_domain(&host);

        if !self.is_target {
            tracing::debug!("[MITM] 非目标域名 {} → 透传", host);
            return RequestOrResponse::Request(req);
        }

        let uri = req.uri().to_string();
        let method = req.method().as_str().to_string();

        self.req_uri = Some(uri.clone());
        self.req_method = Some(method.clone());
        self.req_start = Some(std::time::Instant::now());
        self.req_body_str = None;

        let req = if self.config.enable_logging {
            let mut headers = std::collections::HashMap::new();
            for (k, v) in req.headers() {
                if let Ok(value) = v.to_str() {
                    headers.insert(k.as_str().to_string(), value.to_string());
                }
            }
            self.req_headers = headers;

            use http_body_util::BodyExt;
            let (parts, body) = req.into_parts();
            match tokio::time::timeout(
                std::time::Duration::from_secs(BODY_COLLECT_TIMEOUT_SECS),
                body.collect(),
            ).await {
                Ok(Ok(collected)) => {
                    let body_bytes = collected.to_bytes();
                    if !body_bytes.is_empty() && body_bytes.len() <= BODY_MAX_SIZE {
                        self.req_body_str = Some(String::from_utf8_lossy(&body_bytes).to_string());
                    }
                    Request::from_parts(parts, Body::from(
                        http_body_util::Full::new(body_bytes),
                    ))
                }
                Ok(Err(e)) => {
                    tracing::warn!("[MITM] 请求体读取失败: {}", e);
                    Request::from_parts(parts, Body::from(http_body_util::Empty::new()))
                }
                Err(_) => {
                    tracing::warn!("[MITM] 请求体读取超时");
                    Request::from_parts(parts, Body::from(http_body_util::Empty::new()))
                }
            }
        } else {
            req
        };

        self.requests_processed.fetch_add(1, Ordering::Relaxed);
        tracing::info!("[MITM] → {} {} (host: {})", method, uri, host);

        RequestOrResponse::Request(req)
    }

    async fn handle_response(
        &mut self,
        _ctx: &HttpContext,
        res: Response<Body>,
    ) -> Response<Body> {
        if !self.is_target {
            return res;
        }

        let uri = self.req_uri.clone().unwrap_or_default();
        let method = self.req_method.clone().unwrap_or_else(|| "*".to_string());
        let req_start = self.req_start;

        if !self.config.enable_logging {
            return res;
        }

        let status = res.status().as_u16();
        let mut response_headers = std::collections::HashMap::new();

        for (k, v) in res.headers() {
            if let Ok(value) = v.to_str() {
                response_headers.insert(k.as_str().to_string(), value.to_string());
            }
        }

        let content_type = response_headers
            .get("content-type")
            .cloned()
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let is_sse = content_type.contains("text/event-stream")
            || content_type.contains("application/x-ndjson")
            || is_streaming_url(&uri);

        let is_binary_stream = content_type.contains("video/")
            || content_type.contains("audio/");

        if is_binary_stream {
            let duration_ms = req_start.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
            tracing::info!("[MITM] ← {} {} [{}] binary stream ({})", method, uri, status, content_type);
            self.monitor.log_request(
                &method, &uri,
                self.req_headers.clone(), response_headers,
                self.req_body_str.as_deref(), None,
                duration_ms, status,
            ).await;
            return res;
        }

        if is_sse {
            return self.handle_streaming_response(
                res, method, uri, response_headers, status, req_start,
            );
        }

        // Non-streaming: collect entire body
        use http_body_util::BodyExt;
        let (parts, body) = res.into_parts();
        let duration_ms = req_start.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);

        let mut body_clone_str = None;

        let final_res = {
            let collect_result = tokio::time::timeout(
                std::time::Duration::from_secs(BODY_COLLECT_TIMEOUT_SECS),
                body.collect(),
            ).await;

            match collect_result {
                Ok(Ok(collected)) => {
                    let body_bytes = collected.to_bytes();
                    if body_bytes.len() <= BODY_MAX_SIZE {
                        use crate::mitm::parser::decode_response_body;
                        body_clone_str = Some(decode_response_body(&body_bytes, &response_headers));
                    }
                    tracing::info!("[MITM] ← {} {} [{}] {} bytes", method, uri, status, body_bytes.len());
                    Response::from_parts(parts, Body::from(
                        http_body_util::Full::new(bytes::Bytes::from(body_bytes)),
                    ))
                }
                Ok(Err(e)) => {
                    tracing::warn!("[MITM] ← {} {} body collect error: {}", method, uri, e);
                    Response::from_parts(parts, Body::from(http_body_util::Empty::new()))
                }
                Err(_) => {
                    tracing::warn!("[MITM] ← {} {} body collect timeout ({}s)",
                        method, uri, BODY_COLLECT_TIMEOUT_SECS);
                    body_clone_str = Some("[body collect timeout]".to_string());
                    Response::from_parts(parts, Body::from(http_body_util::Empty::new()))
                }
            }
        };

        self.monitor.log_request(
            &method, &uri,
            self.req_headers.clone(), response_headers,
            self.req_body_str.as_deref(), body_clone_str.as_deref(),
            duration_ms, status,
        ).await;

        final_res
    }
}
