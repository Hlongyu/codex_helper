//! Responses WebSocket transport. Native sockets stay pinned to one provider;
//! HTTP adapters expand connection-local continuation history before routing.
use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures_util::{stream::FuturesUnordered, SinkExt};
use tokio_tungstenite::{
    tungstenite::{
        handshake::{client::generate_key, derive_accept_key},
        protocol::Role,
        Message as UpstreamMessage,
    },
    WebSocketStream,
};

const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_STREAMS: usize = 32;
type UpstreamSocket = WebSocketStream<reqwest::Upgraded>;

/// Maintenance probe: deliberately does not fall back or write routed-call logs.
pub(super) async fn measure_provider(
    provider: &ProviderConfig,
    client: reqwest::Client,
    base_url: String,
    token: String,
    mut body: Value,
) -> Result<ProviderLatencyProbe, String> {
    let started = Instant::now();
    let candidate = UpstreamCandidate {
        provider: provider.clone(),
        base_url,
        token,
        route_order: 1,
    };
    let client = UpstreamClient {
        client,
        response_header_timeout_secs: 60,
    };
    body.as_object_mut().unwrap().remove("stream");
    body["type"] = json!("response.create");
    let request = gpt6_compatible_request_body(body.to_string().as_bytes(), "responses");
    tokio::time::timeout(Duration::from_secs(60), async {
        let (mut socket, _) = connect(
            &client,
            &candidate,
            &HeaderMap::new(),
            None,
            &request_id(current_epoch_ms().unwrap_or_default()),
        )
        .await
        .map_err(|(_, error)| error)?;
        socket
            .send(UpstreamMessage::Text(
                String::from_utf8_lossy(&request).into_owned().into(),
            ))
            .await
            .map_err(|_| "发送 WS 测试请求失败")?;
        let mut reply = String::new();
        while let Some(message) = socket.next().await {
            let event: Value = match message {
                Ok(UpstreamMessage::Text(text)) => {
                    serde_json::from_str(&text).map_err(|_| "WS 测试返回无效 JSON")?
                }
                Ok(UpstreamMessage::Binary(bytes)) => {
                    serde_json::from_slice(&bytes).map_err(|_| "WS 测试返回无效 JSON")?
                }
                Ok(UpstreamMessage::Ping(_)) => {
                    socket.flush().await.map_err(|_| "WS 测试心跳失败")?;
                    continue;
                }
                Ok(UpstreamMessage::Pong(_)) => continue,
                _ => return Err("WS 测试在响应完成前断开".into()),
            };
            if let Some(delta) = reply_from_stream_event(&event, LatencyResponseFormat::Responses) {
                reply.push_str(&delta);
            }
            if event["type"] == "response.completed" {
                if reply.is_empty() {
                    reply = reply_from_response_value(
                        &event["response"],
                        LatencyResponseFormat::Responses,
                    );
                }
                let _ = socket.close(None).await;
                return Ok(ProviderLatencyProbe {
                    latency_ms: started.elapsed().as_millis() as u64,
                    reply,
                });
            }
            if is_terminal(&event) {
                return Err(event
                    .pointer("/error/message")
                    .or_else(|| event.pointer("/response/error/message"))
                    .and_then(Value::as_str)
                    .unwrap_or("WS 测试响应失败")
                    .to_string());
            }
        }
        Err("WS 测试在响应完成前断开".into())
    })
    .await
    .map_err(|_| "WS 模型测试超过 60 秒".to_string())?
}

pub(super) async fn upgrade(
    AxumState(proxy): AxumState<Arc<ProxyState>>,
    uri: Uri,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    // Authenticate before accepting an upgraded connection or contacting an upstream.
    let (router, _) = match upstream_candidates() {
        Ok(config) => config,
        Err(error) => return proxy_error(StatusCode::BAD_GATEWAY, error),
    };
    if bearer_token(&headers) != Some(router.local_token.trim()) {
        return proxy_error(StatusCode::UNAUTHORIZED, "本地路由 Token 无效");
    }
    if !router.websockets_enabled {
        return proxy_error(
            StatusCode::UPGRADE_REQUIRED,
            "请先在 Codex 路由设置中开启 WebSocket",
        );
    }
    let ws = match ws {
        Ok(ws) => ws,
        Err(_) => return proxy_error(StatusCode::BAD_REQUEST, "需要有效的 WebSocket Upgrade 请求"),
    };
    #[cfg(test)]
    let test_dir = TEST_MANAGER_DIR.try_with(Clone::clone).ok();
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
            #[cfg(test)]
            if let Some(dir) = test_dir {
                TEST_MANAGER_DIR
                    .scope(dir, session(socket, proxy, uri, headers, router))
                    .await;
                return;
            }
            session(socket, proxy, uri, headers, router).await;
        })
}

fn stream_key(event: &Value) -> String {
    event
        .get("stream_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn error_event(code: &str, message: impl Into<String>, stream_id: &str) -> Value {
    let upstream = code.starts_with("upstream_") || code == "router_unavailable";
    let mut value = json!({"type": "error", "status": if upstream { 502 } else { 400 },
        "error": {"type": if upstream { "server_error" } else { "invalid_request_error" }, "code": code, "message": message.into()}});
    if !stream_id.is_empty() {
        value["stream_id"] = json!(stream_id);
    }
    value
}

async fn send_event(socket: &mut WebSocket, value: &Value) -> Result<(), axum::Error> {
    tokio::time::timeout(
        Duration::from_secs(30),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .map_err(|_| {
        axum::Error::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "客户端接收超时",
        ))
    })?
}

async fn send_upstream_event(
    socket: &mut UpstreamSocket,
    body: String,
    timeout_secs: u64,
) -> Result<(), String> {
    tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        socket.send(UpstreamMessage::Text(body.into())),
    )
    .await
    .map_err(|_| "发送上游 WebSocket 消息超时".to_string())?
    .map_err(|_| "发送上游 WebSocket 消息失败".to_string())
}

fn parse_message(message: Message) -> Result<Option<Value>, String> {
    match message {
        Message::Text(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|_| "WS 消息必须是 JSON".into()),
        Message::Binary(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "WS 消息必须是 JSON".into()),
        _ => Ok(None),
    }
}

fn validate_create(event: &Value) -> Result<(), String> {
    if event.get("type").and_then(Value::as_str) != Some("response.create") {
        return Err("连接的首条消息必须是 response.create".into());
    }
    if event
        .get("model")
        .and_then(Value::as_str)
        .is_none_or(|model| model.trim().is_empty())
    {
        return Err("response.create 缺少 model".into());
    }
    if event.get("stream_id").is_some_and(|id| !id.is_string()) {
        return Err("stream_id 必须是字符串".into());
    }
    Ok(())
}

fn eligible(candidate: &UpstreamCandidate, event: &Value) -> bool {
    let model = event
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    provider_accepts_model(&candidate.provider, model)
        && provider_supports_reasoning_effort(
            &candidate.provider,
            event.pointer("/reasoning/effort").and_then(Value::as_str),
        )
        && provider_responses_requirement(
            &candidate.provider,
            "responses",
            event.to_string().as_bytes(),
            model,
        )
        .is_none()
}

fn native_provider(candidate: &UpstreamCandidate) -> bool {
    candidate.provider.supports_websockets
        && candidate.provider.wire_api == ProviderWireApi::Responses
}

fn forward_header(name: &HeaderName, headers: &HeaderMap) -> bool {
    !matches!(
        name.as_str(),
        "authorization"
            | "host"
            | "content-length"
            | "content-encoding"
            | "content-type"
            | "x-request-id"
            | "cookie"
            | "proxy-authorization"
    ) && !name.as_str().starts_with("sec-websocket-")
        && !is_hop_by_hop_header(name)
        && !headers
            .get_all(CONNECTION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|value| value.trim().eq_ignore_ascii_case(name.as_str()))
}

fn upstream_http_url(base_url: &str, query: Option<&str>) -> Result<String, String> {
    let mut url =
        reqwest::Url::parse(&join_url(base_url, "responses")).map_err(|_| "上游 URL 无效")?;
    match url.scheme() {
        "ws" => {
            url.set_scheme("http").map_err(|_| "上游 URL 无效")?;
        }
        "wss" => {
            url.set_scheme("https").map_err(|_| "上游 URL 无效")?;
        }
        "http" | "https" => (),
        _ => return Err("WebSocket 上游需要 HTTP(S) 或 WS(S) 地址".into()),
    }
    if let Some(query) = query {
        url.set_query(Some(query));
    }
    Ok(url.into())
}

// reqwest performs HTTP CONNECT/TLS and retains the app's system-proxy behavior.
// tungstenite owns framing only after the HTTP upgrade has been validated.
async fn connect(
    client: &UpstreamClient,
    candidate: &UpstreamCandidate,
    headers: &HeaderMap,
    query: Option<&str>,
    request_id: &str,
) -> Result<(UpstreamSocket, HeaderMap), (Option<u16>, String)> {
    let url = upstream_http_url(&candidate.base_url, query).map_err(|error| (None, error))?;
    let key = generate_key();
    let mut request = client.client.get(url).version(reqwest::Version::HTTP_11);
    for (name, value) in headers {
        if forward_header(name, headers) {
            request = request.header(name, value);
        }
    }
    let response = send_upstream_request(
        request
            .header(AUTHORIZATION, format!("Bearer {}", candidate.token))
            .header("x-request-id", request_id)
            .header(CONNECTION, "Upgrade")
            .header(UPGRADE, "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", &key),
        client.response_header_timeout_secs,
    )
    .await
    .map_err(|error| (None, error))?;
    let status = response.status().as_u16();
    if status != 101 {
        return Err((Some(status), format!("WebSocket 握手返回 HTTP {status}")));
    }
    let response_headers = response.headers().clone();
    let accept = derive_accept_key(key.as_bytes());
    if response_headers
        .get("sec-websocket-accept")
        .and_then(|value| value.to_str().ok())
        != Some(accept.as_str())
        || !response_headers
            .get(UPGRADE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        || !response_headers
            .get(CONNECTION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("upgrade"))
            })
        || response_headers.contains_key("sec-websocket-extensions")
        || response_headers.contains_key("sec-websocket-protocol")
    {
        return Err((Some(status), "上游 WebSocket 握手响应无效".into()));
    }
    let stream = response
        .upgrade()
        .await
        .map_err(|_| (Some(status), "无法升级上游 WebSocket 连接".into()))?;
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    Ok((
        WebSocketStream::from_raw_socket(stream, Role::Client, Some(config)).await,
        response_headers,
    ))
}

async fn session(
    mut socket: WebSocket,
    proxy: Arc<ProxyState>,
    uri: Uri,
    headers: HeaderMap,
    router: RouterConfig,
) {
    let first = loop {
        let message = match tokio::time::timeout(
            Duration::from_secs(router.stream_idle_timeout_secs),
            socket.recv(),
        )
        .await
        {
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Err(_) | Ok(Some(Err(_))) => return,
            Ok(Some(Ok(Message::Ping(data)))) => {
                if socket.send(Message::Pong(data)).await.is_err() {
                    return;
                }
                continue;
            }
            Ok(Some(Ok(message))) => message,
        };
        match parse_message(message) {
            Ok(Some(event)) => match validate_create(&event) {
                Ok(()) => break event,
                Err(error) => {
                    if send_event(
                        &mut socket,
                        &error_event("invalid_request", error, &stream_key(&event)),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
            },
            Err(error) => {
                if send_event(&mut socket, &error_event("invalid_json", error, ""))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            _ => (),
        }
    };
    let timing = RequestTiming::now();
    let config = upstream_candidates();
    let (router, candidates) = match config {
        Ok((router, candidates))
            if bearer_token(&headers) == Some(router.local_token.trim())
                && router.websockets_enabled =>
        {
            (router, candidates)
        }
        _ => {
            let _ = send_event(
                &mut socket,
                &error_event(
                    "router_unavailable",
                    "路由配置已变化，请重新连接",
                    &stream_key(&first),
                ),
            )
            .await;
            return;
        }
    };
    let candidates: Vec<_> = candidates
        .into_iter()
        .filter(|candidate| eligible(candidate, &first))
        .collect();
    let client = match proxy.upstream() {
        Ok(client) => client,
        Err(_) => return,
    };
    let mut chain = Vec::new();
    let mut last_error = "没有可用的上游供应商支持该模型".to_string();
    for (index, candidate) in candidates.iter().enumerate() {
        if !native_provider(candidate) {
            http_session(
                socket,
                proxy,
                uri,
                headers,
                first,
                candidates[index..].to_vec(),
            )
            .await;
            return;
        }
        chain.push(candidate.provider.name.clone());
        let mut call = match NativeCall::new(
            candidate,
            &first,
            timing,
            &router,
            &headers,
            &chain,
            uri.query(),
        ) {
            Ok(call) => call,
            Err(error) => {
                last_error = error;
                break;
            }
        };
        call.guard.pending.as_mut().unwrap().slow_delay_ms = delay(&router).await;
        call.guard.pending.as_mut().unwrap().upstream_started_ms =
            Some(timing.start.elapsed().as_millis() as u64);
        match connect(
            &client,
            candidate,
            &headers,
            uri.query(),
            &call.guard.pending().id,
        )
        .await
        {
            Ok((upstream, response_headers)) => {
                call.connected(&response_headers);
                native_session(
                    socket,
                    upstream,
                    candidate.clone(),
                    router,
                    headers,
                    uri,
                    first,
                    call,
                )
                .await;
                return;
            }
            // A server that doesn't implement Upgrade can still serve HTTP Responses.
            Err((Some(400 | 404 | 405 | 426 | 501), _)) => {
                call.guard.disarm();
                http_session(
                    socket,
                    proxy,
                    uri,
                    headers,
                    first,
                    candidates[index..].to_vec(),
                )
                .await;
                return;
            }
            Err((status, error)) => {
                last_error = format!("{}: {error}", candidate.provider.name);
                call.fail(last_error.clone(), status);
            }
        }
    }
    let _ = send_event(
        &mut socket,
        &error_event("upstream_unavailable", last_error, &stream_key(&first)),
    )
    .await;
}

async fn delay(router: &RouterConfig) -> Option<u64> {
    if router.slow_mode != SlowMode::Off {
        let seconds = selected_slow_delay_secs(router);
        tokio::time::sleep(Duration::from_secs(seconds)).await;
        return Some(seconds.saturating_mul(1000));
    }
    None
}

struct NativeCall {
    guard: PendingRouteLogGuard,
    body: String,
    usage: TokenUsage,
    first_byte_ms: Option<u64>,
    last_activity: tokio::time::Instant,
    generation: bool,
}

impl NativeCall {
    fn new(
        candidate: &UpstreamCandidate,
        event: &Value,
        timing: RequestTiming,
        router: &RouterConfig,
        headers: &HeaderMap,
        chain: &[String],
        query: Option<&str>,
    ) -> Result<Self, String> {
        validate_create(event)?;
        let original = event.to_string();
        let model = event["model"].as_str().unwrap();
        let prepared = prepare_upstream_request(
            &candidate.provider,
            "responses",
            "",
            original.as_bytes(),
            model,
        )?;
        let mut value: Value =
            serde_json::from_slice(&prepared.body).map_err(|_| "无法转换 WS 请求")?;
        value.as_object_mut().unwrap().remove("stream");
        value.as_object_mut().unwrap().remove("background");
        let body = value.to_string();
        let id = request_id(timing.started_at_ms);
        let info = request_body_info(
            headers,
            original.as_bytes(),
            original.as_bytes(),
            body.as_bytes(),
            false,
        );
        let debug = build_pending_debug_capture(
            router.debug_mode,
            &id,
            timing.started_at_ms,
            &candidate.provider.name,
            &Method::GET,
            "/v1/responses".into(),
            headers,
            original.as_bytes(),
            upstream_http_url(&candidate.base_url, query)?,
            body.as_bytes(),
            false,
            info.clone(),
        );
        let mut audit = remote_compaction_v2_audit_from_request_body(original.as_bytes());
        audit.trigger_forwarded = request_has_compaction_trigger(body.as_bytes());
        let mut guard = PendingRouteLogGuard::default();
        let mut pending = build_pending_route_log(
            id,
            timing,
            candidate,
            &Method::GET,
            "responses",
            model,
            audit,
            prepared.upstream_model.as_deref(),
            chain,
            Some(101),
            chain.len(),
            None,
            None,
            None,
            Some(info),
            service_tier_from_request_body(body.as_bytes()),
            debug,
            None,
        );
        pending.route_result.push_str(" · WebSocket");
        guard.arm(pending);
        Ok(Self {
            guard,
            body,
            usage: TokenUsage::default(),
            first_byte_ms: None,
            last_activity: tokio::time::Instant::now(),
            generation: event.get("generate").and_then(Value::as_bool) != Some(false),
        })
    }

    fn connected(&mut self, headers: &HeaderMap) {
        let pending = self.guard.pending.as_mut().unwrap();
        pending.response_header_ms = Some(pending.start.elapsed().as_millis() as u64);
        pending.upstream_request_id = upstream_response_request_id(headers);
        if let Some(debug) = pending.debug_capture.as_mut() {
            debug.upstream_status_code = Some(101);
            debug.upstream_response_headers = debug_headers(headers);
        }
    }

    fn ingest(&mut self, event: &Value) -> bool {
        self.last_activity = tokio::time::Instant::now();
        let pending = self.guard.pending.as_mut().unwrap();
        self.first_byte_ms
            .get_or_insert(pending.start.elapsed().as_millis() as u64);
        merge_token_usage(&mut self.usage, usage_from_response_value(event));
        if let Some(tier) = service_tier_from_response_value(event) {
            pending.actual_service_tier = Some(tier);
        }
        if value_contains_compaction_item(event) {
            pending.remote_compaction_v2.compaction_response_received = true;
            pending.remote_compaction_v2.compaction_response_forwarded = true;
        }
        if let Some(debug) = pending.debug_capture.as_mut() {
            debug
                .upstream_response_body
                .append(format!("{}\n", event).as_bytes());
        }
        match event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "response.completed" => {
                let pending = self.guard.take();
                record_provider_success(ProviderKind::Codex, &pending.provider_id);
                if self.generation {
                    finish_route_log(pending, true, self.usage.clone(), self.first_byte_ms);
                }
                true
            }
            "response.cancelled" => {
                let pending = self.guard.take();
                if self.generation {
                    finish_cancelled_route_log(pending, self.usage.clone(), self.first_byte_ms);
                }
                true
            }
            "error" | "response.failed" | "response.incomplete" => {
                let message = event
                    .pointer("/error/message")
                    .or_else(|| event.pointer("/response/error/message"))
                    .and_then(Value::as_str)
                    .unwrap_or("WebSocket 响应未成功完成")
                    .to_string();
                self.fail(
                    message,
                    Some(
                        event
                            .get("status")
                            .and_then(Value::as_u64)
                            .and_then(|n| u16::try_from(n).ok())
                            .unwrap_or(400),
                    ),
                );
                true
            }
            _ => false,
        }
    }

    fn fail(&mut self, error: String, status: Option<u16>) {
        let mut pending = self.guard.take();
        pending.error = Some(error.clone());
        pending.status_code = status;
        // Validation errors are attributable to the request, not provider availability.
        if status.is_none() || matches!(status, Some(401 | 403 | 429 | 500..=599)) {
            record_provider_failure(
                ProviderKind::Codex,
                &pending.provider_id,
                &pending.provider_name,
                &pending.id,
                pending.started_at_ms,
                &pending.path,
                &pending.model,
                provider_failure_kind_for_status(status),
                status,
                error,
            );
        }
        if self.generation {
            finish_route_log(pending, false, self.usage.clone(), self.first_byte_ms);
        }
    }
}

impl Drop for NativeCall {
    fn drop(&mut self) {
        if let Some(pending) = self.guard.pending.take() {
            if self.generation {
                finish_cancelled_route_log(pending, self.usage.clone(), self.first_byte_ms);
            }
        }
    }
}

async fn native_session(
    mut socket: WebSocket,
    mut upstream: UpstreamSocket,
    candidate: UpstreamCandidate,
    router: RouterConfig,
    headers: HeaderMap,
    uri: Uri,
    first: Value,
    mut call: NativeCall,
) {
    if send_upstream_event(
        &mut upstream,
        call.body.clone(),
        router.stream_idle_timeout_secs,
    )
    .await
    .is_err()
    {
        call.fail("发送上游 WebSocket 消息失败".into(), None);
        let _ = send_event(
            &mut socket,
            &error_event(
                "upstream_disconnected",
                "发送上游 WebSocket 消息失败",
                &stream_key(&first),
            ),
        )
        .await;
        return;
    }
    call.last_activity = tokio::time::Instant::now();
    let mut calls =
        BTreeMap::from([(stream_key(&first), std::collections::VecDeque::from([call]))]);
    let failure = loop {
        let deadline = calls
            .values()
            .filter_map(|lane| lane.front())
            .map(|call| call.last_activity + Duration::from_secs(router.stream_idle_timeout_secs))
            .min()
            .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(86400));
        tokio::select! {
            incoming = socket.recv() => {
                let message = match incoming {
                    Some(Ok(Message::Close(frame))) => {
                        let _ = upstream.send(UpstreamMessage::Close(frame.map(|frame| tokio_tungstenite::tungstenite::protocol::CloseFrame { code: frame.code.into(), reason: frame.reason.as_str().into() }))).await;
                        break None;
                    }
                    Some(Ok(Message::Ping(data))) => { if socket.send(Message::Pong(data)).await.is_err() { break None; } continue; }
                    Some(Ok(message)) => message,
                    _ => break None,
                };
                let event = match parse_message(message) {
                    Ok(Some(event)) => event, Ok(None) => continue,
                    Err(error) => { if send_event(&mut socket, &error_event("invalid_json", error, "")).await.is_err() { break None; } continue; }
                };
                let key = stream_key(&event);
                let body = if event.get("type").and_then(Value::as_str) == Some("response.create") {
                    let current = upstream_candidates().ok();
                    let allowed = current.as_ref().is_some_and(|(config, candidates)| config.websockets_enabled
                        && bearer_token(&headers) == Some(config.local_token.trim())
                        && candidates.iter().any(|next| next.provider.id == candidate.provider.id && eligible(next, &event)));
                    let error = if !allowed || !eligible(&candidate, &event) { Some("当前连接的供应商不可用于该模型，请重新连接") }
                        else if calls.values().map(|lane| lane.len()).sum::<usize>() >= MAX_STREAMS { Some("待完成响应数量超过限制") } else { None };
                    if let Some(error) = error {
                        if send_event(&mut socket, &error_event("invalid_request", error, &key)).await.is_err() { break None; } continue;
                    }
                    let mut next = match NativeCall::new(&candidate, &event, RequestTiming::now(), &router, &headers, &[candidate.provider.name.clone()], uri.query()) {
                        Ok(next) => next,
                        Err(error) => { if send_event(&mut socket, &error_event("invalid_request", error, &key)).await.is_err() { break None; } continue; }
                    };
                    next.guard.pending.as_mut().unwrap().slow_delay_ms = delay(&router).await;
                    next.guard.pending.as_mut().unwrap().upstream_started_ms = Some(next.guard.pending().start.elapsed().as_millis() as u64);
                    let body = next.body.clone();
                    calls.entry(key).or_default().push_back(next);
                    body
                } else { event.to_string() };
                if let Err(error) = send_upstream_event(&mut upstream, body, router.stream_idle_timeout_secs).await { break Some(error); }
            }
            incoming = upstream.next() => {
                let event = match incoming {
                    Some(Ok(UpstreamMessage::Text(text))) => serde_json::from_str::<Value>(&text),
                    Some(Ok(UpstreamMessage::Binary(bytes))) => serde_json::from_slice::<Value>(&bytes),
                    Some(Ok(UpstreamMessage::Ping(_))) => { if upstream.flush().await.is_err() { break Some("上游 WebSocket 心跳失败".into()); } continue; }
                    Some(Ok(UpstreamMessage::Pong(_))) | Some(Ok(UpstreamMessage::Frame(_))) => continue,
                    Some(Ok(UpstreamMessage::Close(frame))) => {
                        let _ = socket.send(Message::Close(frame.map(|frame| axum::extract::ws::CloseFrame { code: frame.code.into(), reason: frame.reason.as_str().into() }))).await;
                        break Some("上游 WebSocket 已关闭".into());
                    }
                    _ => break Some("上游 WebSocket 连接中断".into()),
                };
                let event = match event { Ok(event) => event, Err(_) => break Some("上游 WebSocket 返回无效 JSON".into()) };
                let key = stream_key(&event);
                if send_event(&mut socket, &event).await.is_err() { break None; }
                if let Some(lane) = calls.get_mut(&key) {
                    if lane.front_mut().is_some_and(|call| call.ingest(&event)) {
                        lane.pop_front();
                        if let Some(next) = lane.front_mut() { next.last_activity = tokio::time::Instant::now(); }
                    }
                    if lane.is_empty() { calls.remove(&key); }
                }
            }
            _ = tokio::time::sleep_until(deadline), if !calls.is_empty() => {
                break Some("等待上游 WebSocket 响应超时".into());
            }
        }
    };
    if let Some(error) = failure {
        for (key, lane) in calls {
            for mut call in lane {
                call.fail(error.clone(), None);
            }
            let _ = send_event(
                &mut socket,
                &error_event("upstream_disconnected", &error, &key),
            )
            .await;
        }
    }
    // Dropping active calls on a client disconnect records cancellation. Never replay
    // a response.create after it has been sent: that could duplicate generation.
}

#[derive(Default)]
struct History {
    entries: std::collections::VecDeque<(String, Vec<Value>, usize)>,
    bytes: usize,
}

impl History {
    fn expand(&self, event: &Value) -> Result<Value, String> {
        validate_create(event)?;
        let mut body = event.clone();
        let object = body.as_object_mut().unwrap();
        let mut input = match object
            .remove("previous_response_id")
            .filter(|id| !id.is_null())
        {
            Some(id) => self
                .entries
                .iter()
                .find(|(key, _, _)| Some(key.as_str()) == id.as_str())
                .map(|(_, input, _)| input.clone())
                .ok_or("previous_response_id 不在当前连接缓存中，请清除该 ID 并发送完整上下文")?,
            None => Vec::new(),
        };
        match object.remove("input") {
            Some(Value::Array(items)) => input.extend(items),
            Some(Value::String(text)) => input.push(json!({"role":"user","content":text})),
            None | Some(Value::Null) => (),
            _ => return Err("input 必须是数组或字符串".into()),
        }
        for field in ["type", "stream_id", "generate", "background"] {
            object.remove(field);
        }
        object.insert("input".into(), json!(input));
        object.insert("stream".into(), json!(true));
        if body.to_string().len() > MAX_MESSAGE_BYTES {
            return Err("续接上下文超过 64 MiB，请开启新连接或压缩上下文".into());
        }
        Ok(body)
    }

    fn remember(&mut self, body: &Value, response: &Value) {
        let Some(id) = response.get("id").and_then(Value::as_str) else {
            return;
        };
        let mut input = body
            .get("input")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if let Some(output) = response.get("output").and_then(Value::as_array) {
            input.extend(output.iter().cloned());
        }
        let bytes = serde_json::to_vec(&input)
            .map(|bytes| bytes.len())
            .unwrap_or(MAX_MESSAGE_BYTES + 1);
        if bytes > MAX_MESSAGE_BYTES {
            return;
        }
        while self.entries.len() >= 8 || self.bytes + bytes > MAX_MESSAGE_BYTES {
            if let Some((_, _, size)) = self.entries.pop_front() {
                self.bytes -= size;
            } else {
                break;
            }
        }
        self.bytes += bytes;
        self.entries.push_back((id.to_string(), input, bytes));
    }
}

struct HttpEvent {
    key: String,
    event: Value,
    history: Option<(Value, Value)>,
}

async fn http_session(
    mut socket: WebSocket,
    proxy: Arc<ProxyState>,
    uri: Uri,
    headers: HeaderMap,
    first: Value,
    initial_candidates: Vec<UpstreamCandidate>,
) {
    let mut history = History::default();
    let mut active = BTreeSet::new();
    let mut jobs = FuturesUnordered::<BoxFuture<'static, ()>>::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<HttpEvent>(16);
    let mut queued = Some(first);
    let mut first_candidates = Some(initial_candidates);
    let mut waiting = std::collections::VecDeque::<Value>::new();
    loop {
        if queued.is_none() {
            if let Some(index) = waiting
                .iter()
                .position(|event| !active.contains(&stream_key(event)))
            {
                queued = waiting.remove(index);
            }
        }
        if let Some(event) = queued.take() {
            let key = stream_key(&event);
            let error: Option<String> =
                if event.get("type").and_then(Value::as_str) != Some("response.create") {
                    Some("HTTP 上游仅支持 response.create；实时控制事件需要 WS 上游".into())
                } else if active.len() + waiting.len() >= MAX_STREAMS {
                    Some("待完成响应数量超过限制".into())
                } else {
                    None
                };
            if let Some(error) = error {
                if send_event(&mut socket, &error_event("invalid_request", error, &key))
                    .await
                    .is_err()
                {
                    break;
                }
                continue;
            }
            if active.contains(&key) {
                waiting.push_back(event);
                continue;
            }
            let body = match history.expand(&event) {
                Ok(body) => body,
                Err(error) => {
                    let code = if event.get("previous_response_id").is_some() {
                        "previous_response_not_found"
                    } else {
                        "invalid_request"
                    };
                    if send_event(&mut socket, &error_event(code, error, &key))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }
            };
            let (router, mut candidates) = match upstream_candidates() {
                Ok(config)
                    if config.0.websockets_enabled
                        && bearer_token(&headers) == Some(config.0.local_token.trim()) =>
                {
                    config
                }
                _ => {
                    let _ = send_event(
                        &mut socket,
                        &error_event("router_unavailable", "路由配置已变化，请重新连接", &key),
                    )
                    .await;
                    break;
                }
            };
            if let Some(initial) = first_candidates.take() {
                candidates.retain(|next| {
                    initial
                        .iter()
                        .any(|old| old.provider.id == next.provider.id)
                });
            }
            if event.get("generate").and_then(Value::as_bool) == Some(false) {
                let response = json!({"id":format!("resp_warmup_{}", request_id(current_epoch_ms().unwrap_or_default())),
                    "object":"response", "status":"completed", "model":event["model"], "output":[]});
                history.remember(&body, &response);
                let mut completed = json!({"type":"response.completed", "response":response});
                if !key.is_empty() {
                    completed["stream_id"] = json!(key);
                }
                if send_event(&mut socket, &completed).await.is_err() {
                    break;
                }
                continue;
            }
            active.insert(key.clone());
            jobs.push(
                http_turn(
                    proxy.clone(),
                    uri.clone(),
                    headers.clone(),
                    router,
                    candidates,
                    body,
                    key,
                    tx.clone(),
                )
                .boxed(),
            );
        }
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(Message::Ping(data))) => { if socket.send(Message::Pong(data)).await.is_err() { break; } }
                Some(Ok(message)) => match parse_message(message) {
                    Ok(event) => queued = event,
                    Err(error) => { if send_event(&mut socket, &error_event("invalid_json", error, "")).await.is_err() { break; } }
                }
            },
            Some(message) = rx.recv() => {
                let terminal = is_terminal(&message.event);
                if let Some((body, response)) = message.history { history.remember(&body, &response); }
                if send_event(&mut socket, &message.event).await.is_err() { break; }
                if terminal {
                    active.remove(&message.key);
                }
            }
            Some(()) = jobs.next(), if !jobs.is_empty() => (),
        }
    }
    // Dropping futures drops HTTP response streams and their cancellation guards.
}

fn is_terminal(event: &Value) -> bool {
    matches!(
        event.get("type").and_then(Value::as_str),
        Some(
            "response.completed"
                | "response.failed"
                | "response.incomplete"
                | "response.cancelled"
                | "error"
        )
    )
}

#[derive(Default)]
struct SseDecoder {
    buffer: Vec<u8>,
}

impl SseDecoder {
    fn push(&mut self, bytes: &[u8], eof: bool) -> Result<Vec<Value>, String> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > MAX_MESSAGE_BYTES {
            return Err("上游 SSE 事件超过 64 MiB".into());
        }
        let mut events = Vec::new();
        loop {
            let boundary = self
                .buffer
                .windows(2)
                .position(|bytes| bytes == b"\n\n")
                .map(|index| (index, 2))
                .into_iter()
                .chain(
                    self.buffer
                        .windows(4)
                        .position(|bytes| bytes == b"\r\n\r\n")
                        .map(|index| (index, 4)),
                )
                .min_by_key(|(index, _)| *index);
            let (end, delimiter) = match boundary {
                Some(boundary) => boundary,
                None if eof && !self.buffer.is_empty() => (self.buffer.len(), 0),
                None => break,
            };
            let text =
                std::str::from_utf8(&self.buffer[..end]).map_err(|_| "上游 SSE 包含无效 UTF-8")?;
            let data = text
                .lines()
                .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                .collect::<Vec<_>>()
                .join("\n");
            if !data.trim().is_empty() && data.trim() != "[DONE]" {
                events.push(serde_json::from_str(&data).map_err(|_| "上游 SSE 包含无效 JSON")?);
            }
            self.buffer.drain(..end + delimiter);
        }
        Ok(events)
    }
}

#[allow(clippy::too_many_arguments)]
async fn http_turn(
    proxy: Arc<ProxyState>,
    uri: Uri,
    headers: HeaderMap,
    router: RouterConfig,
    candidates: Vec<UpstreamCandidate>,
    body: Value,
    key: String,
    tx: tokio::sync::mpsc::Sender<HttpEvent>,
) {
    let result = http_turn_inner(proxy, uri, headers, router, candidates, &body, &key, &tx).await;
    if let Err(error) = result {
        let _ = tx
            .send(HttpEvent {
                key: key.clone(),
                event: error_event("upstream_error", error, &key),
                history: None,
            })
            .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn http_turn_inner(
    proxy: Arc<ProxyState>,
    uri: Uri,
    headers: HeaderMap,
    router: RouterConfig,
    candidates: Vec<UpstreamCandidate>,
    body: &Value,
    key: &str,
    tx: &tokio::sync::mpsc::Sender<HttpEvent>,
) -> Result<(), String> {
    let mut http_headers = HeaderMap::new();
    for (name, value) in &headers {
        if forward_header(name, &headers) {
            http_headers.insert(name.clone(), value.clone());
        }
    }
    http_headers.insert(
        AUTHORIZATION,
        headers
            .get(AUTHORIZATION)
            .cloned()
            .ok_or("缺少本地 Token")?,
    );
    http_headers.insert("content-type", HeaderValue::from_static("application/json"));
    http_headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    let mut timing = RequestTiming::now();
    if router.slow_mode != SlowMode::Off {
        let seconds = selected_slow_delay_secs(&router);
        timing.slow_delay_ms = Some(seconds.saturating_mul(1000));
        tokio::time::sleep(Duration::from_secs(seconds)).await;
    }
    let response = proxy_request_with_candidates(
        proxy,
        Method::POST,
        uri,
        http_headers,
        "responses".into(),
        Body::from(body.to_string()),
        Some(timing),
        Some((router, candidates)),
    )
    .await;
    if !response.status().is_success() {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), MAX_MESSAGE_BYTES)
            .await
            .map_err(|_| "读取 HTTP 错误响应失败")?;
        let value: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        return Err(format!(
            "HTTP {status}: {}",
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("上游请求失败")
        ));
    }
    let is_sse = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"));
    if !is_sse {
        let bytes = axum::body::to_bytes(response.into_body(), MAX_MESSAGE_BYTES)
            .await
            .map_err(|_| "读取 HTTP 响应失败")?;
        let response: Value =
            serde_json::from_slice(&bytes).map_err(|_| "上游未返回有效 Responses JSON")?;
        if response.get("object").and_then(Value::as_str) != Some("response") {
            return Err("上游未返回 Responses 对象".into());
        }
        let event_type = match response.get("status").and_then(Value::as_str) {
            Some("completed") => "response.completed",
            Some("failed") => "response.failed",
            Some("incomplete") => "response.incomplete",
            _ => return Err("上游 JSON 响应尚未完成".into()),
        };
        return emit_http_event(
            tx,
            key,
            body,
            json!({"type":event_type, "response":response}),
        )
        .await;
    }
    let mut stream = response.into_body().into_data_stream();
    let mut decoder = SseDecoder::default();
    while let Some(bytes) = stream.next().await {
        let bytes = bytes.map_err(|_| "读取上游 HTTP 流失败")?;
        for event in decoder.push(&bytes, false)? {
            let terminal = is_terminal(&event);
            emit_http_event(tx, key, body, event).await?;
            if terminal {
                return Ok(());
            }
        }
    }
    for event in decoder.push(&[], true)? {
        let terminal = is_terminal(&event);
        emit_http_event(tx, key, body, event).await?;
        if terminal {
            return Ok(());
        }
    }
    Err("上游 HTTP 流在响应完成前断开".into())
}

async fn emit_http_event(
    tx: &tokio::sync::mpsc::Sender<HttpEvent>,
    key: &str,
    body: &Value,
    mut event: Value,
) -> Result<(), String> {
    if !key.is_empty() {
        event["stream_id"] = json!(key);
    }
    let history = (event.get("type").and_then(Value::as_str) == Some("response.completed"))
        .then(|| {
            event
                .get("response")
                .cloned()
                .map(|response| (body.clone(), response))
        })
        .flatten();
    tx.send(HttpEvent {
        key: key.into(),
        event,
        history,
    })
    .await
    .map_err(|_| "客户端已断开".into())
}

#[cfg(test)]
mod tests;
