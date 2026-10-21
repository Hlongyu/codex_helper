//! Browser-to-desktop provider imports. Secrets stay in Rust until confirmation.
use super::*;
use std::collections::VecDeque;
use tauri::Emitter;
use tauri_plugin_deep_link::DeepLinkExt;

const EVENT: &str = "provider-import-pending";
const MAX_LINK_BYTES: usize = 16 * 1024;
const MAX_PENDING: usize = 16;

#[derive(Clone)]
struct ImportRequest {
    app: String,
    name: String,
    endpoint: String,
    api_key: String,
    api_format: String,
    models: Vec<String>,
    enabled: bool,
}

#[derive(Clone, Serialize)]
pub(crate) struct ImportPreview {
    id: u64,
    app: String,
    name: String,
    endpoint: String,
    api_key_masked: String,
    api_format: String,
    models: Vec<String>,
    enabled: bool,
    error: Option<String>,
}

struct PendingImport {
    id: u64,
    fingerprint: Vec<u8>,
    request: Result<ImportRequest, String>,
}

#[derive(Default)]
pub(crate) struct ImportInbox {
    queue: Mutex<VecDeque<PendingImport>>,
    sequence: AtomicU64,
}

impl ImportInbox {
    fn receive(&self, raw: &str) {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let fingerprint = Sha256::digest(raw.as_bytes()).to_vec();
        // The OS and plugin may deliver the cold-start URL more than once.
        if queue.iter().any(|entry| entry.fingerprint == fingerprint) || queue.len() >= MAX_PENDING
        {
            return;
        }
        queue.push_back(PendingImport {
            id: self.sequence.fetch_add(1, Ordering::Relaxed) + 1,
            fingerprint,
            request: parse_link(raw),
        });
    }

    fn confirm(
        &self,
        id: u64,
        enabled: bool,
        persist: impl FnOnce(&ImportRequest, bool) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut queue = self.queue.lock().map_err(|_| "导入队列暂不可用")?;
        let entry = queue
            .front()
            .filter(|entry| entry.id == id)
            .ok_or("导入请求已处理，请重新打开链接")?;
        let request = entry.request.as_ref().map_err(Clone::clone)?;
        persist(request, enabled)?;
        // Consume only after persistence succeeds; retry on failure, never duplicate on double-click.
        queue.pop_front();
        Ok(())
    }

    fn preview(&self) -> Option<ImportPreview> {
        let queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.front().map(|entry| match &entry.request {
            Ok(request) => ImportPreview {
                id: entry.id,
                app: request.app.clone(),
                name: request.name.clone(),
                endpoint: request.endpoint.clone(),
                // Never expose a complete short key, or the raw link, to the webview.
                api_key_masked: if request.api_key.chars().count() > 8 {
                    format!(
                        "••••••••{}",
                        request
                            .api_key
                            .chars()
                            .rev()
                            .take(4)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<String>()
                    )
                } else {
                    "••••••••".to_string()
                },
                api_format: request.api_format.clone(),
                models: request.models.clone(),
                enabled: request.enabled,
                error: None,
            },
            Err(error) => ImportPreview {
                id: entry.id,
                app: String::new(),
                name: String::new(),
                endpoint: String::new(),
                api_key_masked: String::new(),
                api_format: String::new(),
                models: vec![],
                enabled: false,
                error: Some(error.clone()),
            },
        })
    }
}

fn parse_link(raw: &str) -> Result<ImportRequest, String> {
    if raw.len() > MAX_LINK_BYTES {
        return Err("导入链接过长（最多 16 KiB）".into());
    }
    let url = reqwest::Url::parse(raw).map_err(|_| "导入链接格式无效")?;
    if url.scheme() != "xxswitch"
        || url.host_str() != Some("v1")
        || url.path() != "/import"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err("仅支持 xxswitch://v1/import 供应商导入链接".into());
    }
    let mut params = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        if !matches!(
            key.as_ref(),
            "resource" | "app" | "name" | "endpoint" | "apiKey" | "apiFormat" | "model" | "enabled"
        ) {
            return Err("链接含有当前版本不支持的参数；请使用基础供应商字段，不要携带 config、脚本或远程配置".into());
        }
        if params
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return Err("导入链接包含重复参数".into());
        }
    }
    let get = |key: &str| params.get(key).map(|s| s.trim()).unwrap_or("");
    if get("resource") != "provider" {
        return Err("当前仅支持 resource=provider".into());
    }
    let app = get("app");
    if !matches!(app, "codex" | "claude") {
        return Err("app 必须是 codex 或 claude".into());
    }
    let name = get("name");
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err("供应商名称必须为 1–100 个字符，且不能包含控制字符".into());
    }
    let key = get("apiKey");
    if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
        return Err("API Key 不能为空、包含控制字符或超过 4096 字节".into());
    }
    let endpoint_text = get("endpoint");
    let endpoint = reqwest::Url::parse(endpoint_text).map_err(|_| "接口地址无效")?;
    if !matches!(endpoint.scheme(), "http" | "https")
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint_text.contains(',')
        || endpoint_text.chars().any(char::is_control)
    {
        return Err("接口地址须为单个 HTTP(S) 地址，不能包含用户名、密码、查询参数或片段".into());
    }
    let format =
        match (app, get("apiFormat")) {
            ("codex", "" | "openai_responses") => "openai_responses",
            ("codex", "openai_chat") => "openai_chat",
            ("claude", "" | "anthropic") => "anthropic",
            _ => return Err(
                "接口协议不匹配：Codex 支持 openai_responses/openai_chat；Claude 支持 anthropic"
                    .into(),
            ),
        };
    let models = normalize_model_names(get("model").split(',').map(str::to_string).collect());
    if models.len() > 100
        || models
            .iter()
            .any(|m| m.len() > 256 || m.chars().any(char::is_control))
    {
        return Err("模型列表无效：最多 100 个模型，每个最多 256 字节且不能包含控制字符".into());
    }
    let enabled = match params.get("enabled").map(String::as_str) {
        None | Some("false") => false,
        Some("true") => true,
        _ => return Err("enabled 必须为 true 或 false".into()),
    };
    Ok(ImportRequest {
        app: app.into(),
        name: name.into(),
        endpoint: endpoint.to_string().trim_end_matches('/').into(),
        api_key: key.into(),
        api_format: format.into(),
        models,
        enabled,
    })
}

fn append_provider(
    state: &mut ManagerState,
    request: &ImportRequest,
    enabled: bool,
) -> Result<String, String> {
    let id = new_provider_id()?;
    let status = if enabled {
        ProviderStatus::Enabled
    } else {
        ProviderStatus::Disabled
    };
    if request.app == "codex" {
        let mut provider = new_codex_provider(id.clone(), request.name.clone())?;
        provider.status = status;
        provider.enabled = enabled;
        provider.wire_api = if request.api_format == "openai_chat" {
            ProviderWireApi::ChatCompletions
        } else {
            ProviderWireApi::Responses
        };
        apply_provider_connection_draft(
            &mut provider,
            Some(&request.endpoint),
            Some(&request.api_key),
        )?;
        provider.connection_test_model = request.models.first().cloned().unwrap_or_default();
        provider.allowed_models = request.models.clone();
        state.providers.push(provider);
    } else {
        let mut provider = new_claude_provider(id.clone(), request.name.clone())?;
        provider.status = status;
        provider.enabled = enabled;
        provider.base_url = request.endpoint.clone();
        provider.api_key = request.api_key.clone();
        provider.connection_test_model = request.models.first().cloned().unwrap_or_default();
        provider.allowed_models = request.models.clone();
        state.claude_providers.push(provider);
    }
    // Append only. Existing providers, route order and active selections are preserved.
    Ok(id)
}

#[tauri::command]
pub(crate) fn peek_provider_import(inbox: tauri::State<ImportInbox>) -> Option<ImportPreview> {
    inbox.preview()
}

#[tauri::command]
pub(crate) fn dismiss_provider_import(
    id: u64,
    inbox: tauri::State<ImportInbox>,
) -> Result<(), String> {
    let mut queue = inbox.queue.lock().map_err(|_| "导入队列暂不可用")?;
    if queue.front().map(|entry| entry.id) != Some(id) {
        return Err("导入请求已处理，请重新打开链接".into());
    }
    queue.pop_front();
    Ok(())
}

#[tauri::command]
pub(crate) fn confirm_provider_import(
    id: u64,
    enabled: bool,
    inbox: tauri::State<ImportInbox>,
) -> Result<(), String> {
    inbox.confirm(id, enabled, |request, enabled| {
        let mut state = load_state_file()?;
        append_provider(&mut state, request, enabled)?;
        save_state(&state)
    })
}

fn receive(app: &tauri::AppHandle, raw: &str) {
    if !raw.to_ascii_lowercase().starts_with("xxswitch:") {
        return;
    }
    app.state::<ImportInbox>().receive(raw);
    show_main_window(app);
    // Event contains no URL or credentials. A late frontend can always read the pending inbox.
    let _ = app.emit(EVENT, ());
}

pub(crate) fn setup(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(any(target_os = "linux", all(debug_assertions, target_os = "windows")))]
    app.deep_link().register_all()?;
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            receive(&handle, url.as_str());
        }
    });
    if let Some(urls) = app.deep_link().get_current()? {
        for url in urls {
            receive(app, url.as_str());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(app: &str, extra: &[(&str, &str)]) -> String {
        let mut url = reqwest::Url::parse("xxswitch://v1/import").unwrap();
        url.query_pairs_mut()
            .extend_pairs([
                ("resource", "provider"),
                ("app", app),
                ("name", "我的供应商 + A&B"),
                ("endpoint", "https://api.example.com/v1/"),
                ("apiKey", "sk-test+secret&value"),
            ])
            .extend_pairs(extra.iter().copied());
        url.to_string()
    }

    #[test]
    fn parses_encoded_key_name_and_models() {
        let request = parse_link(&link(
            "codex",
            &[
                ("apiFormat", "openai_chat"),
                ("model", "model-a,model-b,model-a"),
            ],
        ))
        .unwrap();
        assert_eq!(request.name, "我的供应商 + A&B");
        assert_eq!(request.api_key, "sk-test+secret&value");
        assert_eq!(request.api_format, "openai_chat");
        assert_eq!(request.models, ["model-a", "model-b"]);
        assert_eq!(request.endpoint, "https://api.example.com/v1");
        assert!(!request.enabled);
        assert_eq!(
            parse_link(&link("claude", &[])).unwrap().api_format,
            "anthropic"
        );
    }

    #[test]
    fn rejects_ambiguous_unsupported_and_invalid_links_without_echoing_secrets() {
        let base = link("codex", &[]);
        let cases = [
            base.replace("xxswitch:", "ccswitch:"),
            base.replace("v1/import", "v2/import"),
            base.replace("v1/import", "v1/other"),
            format!("{base}#sk-private"),
            link("gemini", &[]),
            link("codex", &[("apiFormat", "anthropic")]),
            link("claude", &[("apiFormat", "openai_chat")]),
            link("codex", &[("apiKey", "sk-private")]),
            link("codex", &[("config", "sk-private")]),
            link("codex", &[("enabled", "yes")]),
            format!("{base}{}", "x".repeat(MAX_LINK_BYTES)),
            base.replace(
                "https%3A%2F%2Fapi.example.com%2Fv1%2F",
                "file%3A%2F%2F%2Ftmp%2Fsk-private",
            ),
        ];
        for raw in cases {
            let error = parse_link(&raw).err().expect("must reject");
            assert!(!error.contains("sk-private"));
            assert!(!error.contains("sk-test"));
        }
    }

    #[test]
    fn pending_import_survives_reads_and_deduplicates_os_delivery() {
        let inbox = ImportInbox::default();
        let raw = link("codex", &[]);
        inbox.receive(&raw);
        inbox.receive(&raw);
        let first = inbox.preview().unwrap();
        assert_eq!(inbox.preview().unwrap().id, first.id);
        assert_eq!(inbox.queue.lock().unwrap().len(), 1);
        let serialized = serde_json::to_string(&first).unwrap();
        assert!(!serialized.contains("sk-test"));
        inbox.receive(&link("claude", &[]));
        assert_eq!(inbox.preview().unwrap().id, first.id);
        inbox.queue.lock().unwrap().pop_front();
        assert_ne!(inbox.preview().unwrap().id, first.id);
        assert_eq!(inbox.preview().unwrap().app, "claude");
    }

    #[test]
    fn saves_once_after_confirmation_and_retains_failed_requests() {
        let inbox = ImportInbox::default();
        inbox.receive(&link("codex", &[("enabled", "true")]));
        let id = inbox.preview().unwrap().id;
        assert!(inbox
            .confirm(id + 1, true, |_, _| panic!("stale request must not save"))
            .is_err());
        assert!(inbox
            .confirm(id, false, |_, _| Err("disk full".into()))
            .is_err());
        assert_eq!(inbox.preview().unwrap().id, id);
        inbox
            .confirm(id, false, |_, enabled| {
                assert!(!enabled);
                Ok(())
            })
            .unwrap();
        assert!(inbox.preview().is_none());
        assert!(inbox
            .confirm(id, true, |_, _| panic!("double submission must not save"))
            .is_err());
    }

    #[test]
    fn importing_preserves_existing_providers_and_maps_each_protocol() {
        let mut state = default_state();
        let existing = new_codex_provider("existing".into(), "Existing".into()).unwrap();
        let original = serde_json::to_value(&existing).unwrap();
        state.providers.push(existing);
        state.active_provider_id = "existing".into();
        let router_before = serde_json::to_value(&state.router).unwrap();
        let clients_before = serde_json::to_value(&state.clients).unwrap();
        let request = parse_link(&link(
            "codex",
            &[("apiFormat", "openai_chat"), ("model", "model-a")],
        ))
        .unwrap();
        let id = append_provider(&mut state, &request, false).unwrap();
        assert_ne!(id, "existing");
        assert_eq!(serde_json::to_value(&state.providers[0]).unwrap(), original);
        assert_eq!(state.active_provider_id, "existing");
        let added = &state.providers[1];
        assert_eq!(added.status, ProviderStatus::Disabled);
        assert!(!added.enabled);
        assert!(matches!(added.wire_api, ProviderWireApi::ChatCompletions));
        assert_eq!(
            added.config["model_providers"]["custom"]["experimental_bearer_token"],
            request.api_key
        );
        assert_eq!(added.allowed_models, ["model-a"]);
        let claude = parse_link(&link("claude", &[])).unwrap();
        append_provider(&mut state, &claude, true).unwrap();
        assert_eq!(state.claude_providers[0].base_url, claude.endpoint);
        assert_eq!(state.claude_providers[0].api_key, claude.api_key);
        assert!(state.claude_providers[0].enabled);
        assert_eq!(serde_json::to_value(&state.router).unwrap(), router_before);
        assert_eq!(
            serde_json::to_value(&state.clients).unwrap(),
            clients_before
        );
        assert_eq!(
            normalize_state(state).providers[1].status,
            ProviderStatus::Disabled
        );
    }

    #[test]
    fn rejects_missing_fields_and_unsafe_endpoint_forms() {
        let base = reqwest::Url::parse(&link("codex", &[])).unwrap();
        for field in ["app", "resource", "name", "endpoint", "apiKey"] {
            let mut candidate = base.clone();
            candidate.set_query(None);
            candidate
                .query_pairs_mut()
                .extend_pairs(base.query_pairs().filter(|(key, _)| key != field));
            assert!(parse_link(candidate.as_str()).is_err(), "missing {field}");
        }
        for endpoint in [
            "javascript:alert(1)",
            "https://user:secret@example.com/v1",
            "https://api.example.com/v1?key=secret",
            "https://api.example.com/#secret",
            "https://one.example,https://two.example",
        ] {
            let mut candidate = base.clone();
            candidate.set_query(None);
            candidate
                .query_pairs_mut()
                .extend_pairs(base.query_pairs().filter(|(key, _)| key != "endpoint"))
                .append_pair("endpoint", endpoint);
            assert!(parse_link(candidate.as_str()).is_err(), "invalid endpoint");
        }
    }
}
