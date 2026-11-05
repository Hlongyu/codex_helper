use super::*;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

#[derive(Default)]
struct Observed {
    connections: usize,
    headers: Vec<HeaderMap>,
    requests: Vec<Value>,
    http: bool,
}

async fn mock_ws(
    AxumState(observed): AxumState<Arc<Mutex<Observed>>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    {
        let mut observed = observed.lock().unwrap();
        observed.connections += 1;
        observed.headers.push(headers);
        if observed.http {
            return proxy_error(StatusCode::UPGRADE_REQUIRED, "HTTP only");
        }
    }
    ws.on_upgrade(move |mut socket| async move {
        while let Some(Ok(message)) = socket.recv().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
            let Ok(Some(event)) = parse_message(message) else {
                continue;
            };
            if event["type"] != "response.create" {
                let _ =
                    send_event(&mut socket, &json!({"type":"control.echo", "event":event})).await;
                continue;
            }
            let count = {
                let mut observed = observed.lock().unwrap();
                observed.requests.push(event.clone());
                observed.requests.len()
            };
            let mut created =
                json!({"type":"response.created", "response":{"id":format!("resp_{count}")}});
            if let Some(key) = event.get("stream_id") {
                created["stream_id"] = key.clone();
            }
            if send_event(&mut socket, &created).await.is_err() {
                break;
            }
            match event.pointer("/metadata/test_case").and_then(Value::as_str) {
                Some("disconnect") => {
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
                Some("hang") => continue,
                _ => (),
            }
            let mut completed = completion(count);
            if let Some(key) = event.get("stream_id") {
                completed["stream_id"] = key.clone();
            }
            if send_event(&mut socket, &completed).await.is_err() {
                break;
            }
        }
    })
}

fn completion(count: usize) -> Value {
    json!({"type":"response.completed", "response":{"id":format!("resp_{count}"), "object":"response",
        "status":"completed", "model":"mapped-model", "service_tier":"priority",
        "output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"你好"}]}],
        "usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15}}})
}

async fn mock_http(
    AxumState(observed): AxumState<Arc<Mutex<Observed>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let event: Value = serde_json::from_slice(&body).unwrap();
    let disconnect =
        event.pointer("/metadata/test_case").and_then(Value::as_str) == Some("disconnect");
    let count = {
        let mut observed = observed.lock().unwrap();
        observed.headers.push(headers);
        observed.requests.push(event);
        observed.requests.len()
    };
    if disconnect {
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
            ))
            .unwrap();
    }
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(format!(
            "data: {}\r\n\r\ndata: [DONE]\r\n\r\n",
            completion(count)
        )))
        .unwrap()
}

struct Fixture {
    dir: PathBuf,
    address: String,
    observed: Arc<Mutex<Observed>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

impl Fixture {
    async fn new(http_only: bool, native_enabled: bool) -> Self {
        let observed = Arc::new(Mutex::new(Observed {
            http: http_only,
            ..Observed::default()
        }));
        let upstream = Router::new()
            .route("/v1/responses", axum::routing::get(mock_ws).post(mock_http))
            .with_state(observed.clone());
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_url = format!("http://{}/v1", upstream_listener.local_addr().unwrap());
        let upstream_task = tokio::spawn(async move {
            axum::serve(upstream_listener, upstream).await.unwrap();
        });
        let dir = std::env::temp_dir().join(format!(
            "xxswitch-ws-{}",
            request_id(current_epoch_ms().unwrap())
        ));
        fs::create_dir_all(&dir).unwrap();
        let mut state = default_state();
        state.router.enabled = true;
        state.router.websockets_enabled = true;
        state.router.local_token = "test-local-token".into();
        state.router.codex_model = "gpt-5.5".into();
        state.router.debug_mode = true;
        let mut provider = new_codex_provider("ws-test".into(), "WS test".into()).unwrap();
        provider.supports_websockets = native_enabled;
        provider.service_tier = "priority".into();
        provider.image_detail_original_to_auto = true;
        provider.model_mappings = vec![ModelMapping {
            source: "gpt-5.5".into(),
            target: "mapped-model".into(),
        }];
        apply_provider_connection_draft(
            &mut provider,
            Some(&upstream_url),
            Some("test-upstream-token"),
        )
        .unwrap();
        state.providers = vec![provider];
        TEST_MANAGER_DIR
            .scope(dir.clone(), async {
                save_state(&state).unwrap();
            })
            .await;
        let proxy = Arc::new(ProxyState {
            upstream: RwLock::new(UpstreamClient {
                client: reqwest::Client::builder().no_proxy().build().unwrap(),
                response_header_timeout_secs: 2,
            }),
        });
        let test_dir = dir.clone();
        let app = Router::new()
            .route(
                "/v1/responses",
                axum::routing::get(upgrade).fallback(proxy_request),
            )
            .route("/v1/{*path}", any(proxy_request))
            .with_state(proxy)
            .layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let dir = test_dir.clone();
                    async move { TEST_MANAGER_DIR.scope(dir, next.run(request)).await }
                },
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("{}", listener.local_addr().unwrap());
        let local_task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            dir,
            address,
            observed,
            tasks: vec![local_task, upstream_task],
        }
    }

    async fn socket(
        &self,
    ) -> WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
        let mut request = format!("ws://{}/v1/responses?test=query", self.address)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer test-local-token"),
        );
        request.headers_mut().insert(
            "openai-beta",
            HeaderValue::from_static("responses_websockets=2026-02-06"),
        );
        request
            .headers_mut()
            .insert("cookie", HeaderValue::from_static("secret-cookie"));
        tokio_tungstenite::connect_async(request).await.unwrap().0
    }

    async fn logs(&self, count: usize) -> Vec<Value> {
        for _ in 0..100 {
            let logs: Vec<Value> = fs::read_to_string(self.dir.join("route-logs.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if logs.len() >= count {
                return logs;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("expected {count} logs");
    }
}

async fn receive<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    terminal: &str,
) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            if let UpstreamMessage::Text(text) = message {
                let event: Value = serde_json::from_str(&text).unwrap();
                if event["type"] == terminal {
                    return event;
                }
                assert_ne!(event["type"], "error", "{event}");
            } else if matches!(message, UpstreamMessage::Close(_)) {
                panic!("unexpected close");
            }
        }
    })
    .await
    .expect("timed out waiting for event")
}

fn create() -> Value {
    json!({"type":"response.create","model":"gpt-5.5", "stream_id":"main", "store":false,
        "input":[{"role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,YQ==", "detail":"original"}]}]})
}

#[tokio::test]
async fn native_round_trip_keeps_connection_and_records_each_response() {
    let fixture = Fixture::new(false, true).await;
    let mut socket = fixture.socket().await;
    socket
        .send(UpstreamMessage::Text(create().to_string().into()))
        .await
        .unwrap();
    receive(&mut socket, "response.completed").await;
    let second = json!({"type":"response.create","stream_id":"main", "model":"gpt-5.5", "previous_response_id":"resp_1", "input":"继续"});
    socket
        .send(UpstreamMessage::Text(second.to_string().into()))
        .await
        .unwrap();
    assert_eq!(
        receive(&mut socket, "response.completed").await["response"]["id"],
        "resp_2"
    );
    socket
        .send(UpstreamMessage::Text(
            json!({"type":"response.append", "input":"steering"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    receive(&mut socket, "control.echo").await;
    socket.close(None).await.unwrap();
    let logs = fixture.logs(2).await;
    assert!(logs
        .iter()
        .all(|log| log["status"] == "success" && log["total_tokens"] == 15));
    assert!(logs
        .iter()
        .all(|log| log["method"] == "GET" && log["debug_capture_available"] == true));
    let observed = fixture.observed.lock().unwrap();
    assert_eq!(observed.connections, 1);
    assert_eq!(observed.requests[0]["model"], "mapped-model");
    assert_eq!(observed.requests[0]["service_tier"], "priority");
    assert_eq!(
        observed.requests[0]["input"][0]["content"][0]["detail"],
        "auto"
    );
    assert_eq!(observed.requests[1]["previous_response_id"], "resp_1");
    assert_eq!(
        observed.headers[0][AUTHORIZATION],
        "Bearer test-upstream-token"
    );
    assert!(observed.headers[0].contains_key("openai-beta"));
    assert!(!observed.headers[0].contains_key("cookie"));
}

#[tokio::test]
async fn http_fallback_expands_continuation_and_supports_warmup() {
    for native_enabled in [false, true] {
        let fixture = Fixture::new(true, native_enabled).await;
        let mut socket = fixture.socket().await;
        let mut warmup = create();
        warmup["generate"] = json!(false);
        socket
            .send(UpstreamMessage::Text(warmup.to_string().into()))
            .await
            .unwrap();
        let warm = receive(&mut socket, "response.completed").await;
        assert!(fixture.observed.lock().unwrap().requests.is_empty());
        let first = json!({"type":"response.create", "stream_id":"main", "model":"gpt-5.5", "previous_response_id":warm["response"]["id"],"input":"start"});
        socket
            .send(UpstreamMessage::Text(first.to_string().into()))
            .await
            .unwrap();
        receive(&mut socket, "response.completed").await;
        let second = json!({"type":"response.create","stream_id":"main","model":"gpt-5.5","previous_response_id":"resp_1","input":"next"});
        socket
            .send(UpstreamMessage::Text(second.to_string().into()))
            .await
            .unwrap();
        receive(&mut socket, "response.completed").await;
        socket.close(None).await.unwrap();
        let logs = fixture.logs(2).await;
        assert!(logs.iter().all(|log| log["status"] == "success"));
        let observed = fixture.observed.lock().unwrap();
        assert_eq!(observed.connections, usize::from(native_enabled));
        assert_eq!(observed.requests[1]["input"].as_array().unwrap().len(), 4);
        assert!(observed.requests[1].get("previous_response_id").is_none());
        assert!(observed.requests[1].get("type").is_none());
        assert_eq!(observed.requests[1]["stream"], true);
        assert!(!observed
            .headers
            .last()
            .unwrap()
            .contains_key("sec-websocket-key"));
        assert_eq!(
            observed.headers.last().unwrap()[AUTHORIZATION],
            "Bearer test-upstream-token"
        );
    }
}

#[tokio::test]
async fn unauthorized_upgrade_never_contacts_upstream_and_http_route_still_works() {
    let fixture = Fixture::new(false, true).await;
    let error = tokio_tungstenite::connect_async(format!("ws://{}/v1/responses", fixture.address))
        .await
        .unwrap_err();
    assert!(
        matches!(error, tokio_tungstenite::tungstenite::Error::Http(ref response) if response.status() == 401)
    );
    assert_eq!(fixture.observed.lock().unwrap().connections, 0);
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{}/v1/responses", fixture.address))
        .bearer_auth("test-local-token")
        .json(&json!({"model":"gpt-5.5","input":"hello","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response
        .text()
        .await
        .unwrap()
        .contains("response.completed"));
}

#[tokio::test]
async fn native_disconnect_and_client_cancellation_have_distinct_logs() {
    for (mode, status) in [("disconnect", "failed"), ("hang", "cancelled")] {
        let fixture = Fixture::new(false, true).await;
        let mut socket = fixture.socket().await;
        let mut event = create();
        event["metadata"] = json!({"test_case":mode});
        socket
            .send(UpstreamMessage::Text(event.to_string().into()))
            .await
            .unwrap();
        receive(&mut socket, "response.created").await;
        if mode == "hang" {
            socket.close(None).await.unwrap();
        }
        let logs = fixture.logs(1).await;
        assert_eq!(logs[0]["status"], status);
    }
}

#[test]
fn sse_decoder_preserves_split_utf8_and_multiline_events() {
    let data = "data: {\"type\":\"response.completed\",\r\ndata: \"text\":\"你好\"}\r\n\r\ndata: [DONE]\n\n";
    let mut decoder = SseDecoder::default();
    let mut events = Vec::new();
    for byte in data.as_bytes() {
        events.extend(decoder.push(&[*byte], false).unwrap());
    }
    assert_eq!(
        events,
        vec![json!({"type":"response.completed","text":"你好"})]
    );
    assert!(decoder.push(b"data: not-json\n\n", false).is_err());
}

#[tokio::test]
async fn truncated_http_stream_is_an_error_and_failed_log() {
    let fixture = Fixture::new(true, false).await;
    let mut socket = fixture.socket().await;
    let mut event = create();
    event["metadata"] = json!({"test_case":"disconnect"});
    socket
        .send(UpstreamMessage::Text(event.to_string().into()))
        .await
        .unwrap();
    let error = receive(&mut socket, "error").await;
    assert_eq!(error["error"]["code"], "upstream_error");
    assert_eq!(fixture.logs(1).await[0]["status"], "failed");
    assert_eq!(fixture.observed.lock().unwrap().requests.len(), 1);
    socket.close(None).await.unwrap();
}

#[tokio::test]
async fn queued_responses_on_one_lane_are_processed_in_order() {
    for http in [false, true] {
        let fixture = Fixture::new(http, !http).await;
        let mut socket = fixture.socket().await;
        for index in 0..3 {
            let mut event = json!({"type":"response.create","model":"gpt-5.5","stream_id":"main","input":format!("turn {index}")});
            if index > 0 {
                event["previous_response_id"] = json!(format!("resp_{index}"));
            }
            socket
                .send(UpstreamMessage::Text(event.to_string().into()))
                .await
                .unwrap();
        }
        for index in 1..=3 {
            assert_eq!(
                receive(&mut socket, "response.completed").await["response"]["id"],
                format!("resp_{index}")
            );
        }
        socket.close(None).await.unwrap();
        let logs = fixture.logs(3).await;
        assert!(logs.iter().all(|log| log["status"] == "success"));
    }
}

#[tokio::test]
async fn websocket_model_probe_uses_ws_and_reports_unsupported_upgrades() {
    for http in [false, true] {
        let fixture = Fixture::new(http, true).await;
        let provider = TEST_MANAGER_DIR
            .scope(fixture.dir.clone(), async {
                load_state_file().unwrap().providers[0].clone()
            })
            .await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let result = measure_provider(
            &provider,
            client,
            custom_provider_base_url(&provider).unwrap(),
            custom_provider_token(&provider).unwrap(),
            json!({"model":"gpt-5.5","input":"hi","stream":true}),
        )
        .await;
        if http {
            assert!(result.unwrap_err().contains("426"));
        } else {
            assert_eq!(result.unwrap().reply, "你好");
        }
        assert!(!fixture.dir.join("route-logs.jsonl").exists());
    }
}

#[test]
fn history_rejects_unknown_continuation_without_losing_input() {
    let mut history = History::default();
    let body = history.expand(&create()).unwrap();
    history.remember(&body, &completion(1)["response"]);
    let next = json!({"type":"response.create","model":"gpt-5.5","previous_response_id":"resp_1","input":[{"type":"function_call_output","call_id":"call_1","output":"done"}]});
    let expanded = history.expand(&next).unwrap();
    assert_eq!(expanded["input"].as_array().unwrap().len(), 3);
    assert_eq!(expanded["input"][2]["call_id"], "call_1");
    let mut unknown = next;
    unknown["previous_response_id"] = json!("other_connection");
    assert!(history.expand(&unknown).is_err());
}

#[test]
fn websocket_settings_migrate_and_restore_existing_codex_config() {
    for old_value in [None, Some(false), Some(true)] {
        let raw = old_value
            .map(|value| format!("[model_providers.custom]\nsupports_websockets = {value}\n"))
            .unwrap_or_default();
        let doc = raw.parse::<DocumentMut>().unwrap();
        let mut legacy = capture_router_backup(&doc, "custom");
        legacy.schema_version = 3;
        legacy.custom_supports_websockets = RouterFieldBackup::default();
        let router = RouterConfig {
            websockets_enabled: true,
            ..RouterConfig::default()
        };
        let (doc, backup) = prepare_router_patch(doc, Some(&legacy), &router, false).unwrap();
        let applied = render_router_patch_toml(doc, false, &router, false)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(
            toml_path_value(&applied, "model_providers.custom.supports_websockets"),
            Some(json!(true))
        );
        let restored = restore_router_backup(applied, Some(&backup), &router, false)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(
            toml_path_value(&restored, "model_providers.custom.supports_websockets"),
            old_value.map(Value::Bool)
        );
    }
    let provider = new_codex_provider("legacy".into(), "Legacy".into()).unwrap();
    let mut value = serde_json::to_value(provider).unwrap();
    value.as_object_mut().unwrap().remove("supports_websockets");
    assert!(
        !serde_json::from_value::<ProviderConfig>(value)
            .unwrap()
            .supports_websockets
    );
    assert!(
        !serde_json::from_value::<RouterConfig>(json!({}))
            .unwrap()
            .websockets_enabled
    );
}
