//! WebDAV snapshots contain provider configuration only. Device state never leaves this machine.
use super::*;
use reqwest::header::{ETAG, IF_MATCH, IF_NONE_MATCH};

const MAX_BYTES: usize = 4 * 1024 * 1024;
const FORMAT: &str = "xxswitch.providers";
const COMMON_FIELDS: &[&str] = &[
    "id",
    "name",
    "enabled",
    "fusion_enabled",
    "auto_disable_override",
    "auto_disable_enabled",
    "auto_disable_failure_threshold",
    "auto_disable_reset_period",
    "auto_disable_reset_weekday",
    "auto_disable_reset_day",
    "auto_disable_reset_time",
    "connection_test_model",
    "allowed_models",
    "model_mappings",
];
const CODEX_FIELDS: &[&str] = &[
    "config",
    "wire_api",
    "supports_websockets",
    "image_detail_original_to_auto",
    "service_tier",
    "balance_query",
];
const CLAUDE_FIELDS: &[&str] = &["base_url", "api_key"];
static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static PREVIEW: Mutex<Option<PendingPreview>> = Mutex::new(None);
static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static STATE_WRITE_LOCK: Mutex<()> = Mutex::new(());

// Async balance/test requests can finish after a download. Reject stale writes instead
// of letting an old in-memory state silently undo the downloaded provider list.
#[derive(Debug, Default)]
pub(super) struct StateRevision(Mutex<Option<String>>);

impl Clone for StateRevision {
    fn clone(&self) -> Self {
        Self(Mutex::new(
            self.0.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        ))
    }
}

impl StateRevision {
    pub(super) fn from_bytes(bytes: &[u8]) -> Self {
        Self(Mutex::new(Some(format!("{:x}", Sha256::digest(bytes)))))
    }
}

pub(super) fn write_state_checked(
    path: &Path,
    bytes: &[u8],
    revision: &StateRevision,
) -> Result<(), String> {
    let _lock = STATE_WRITE_LOCK.lock().map_err(|_| "状态文件暂不可用")?;
    let mut expected = revision.0.lock().map_err(|_| "状态版本暂不可用")?;
    if let Some(hash) = expected.as_ref() {
        let current = fs::read(path).map_err(|_| "无法检查本机状态版本")?;
        if format!("{:x}", Sha256::digest(&current)) != *hash {
            return Err("本机配置已被其他操作更新，请刷新后重试".into());
        }
    }
    atomic_write(path, bytes)?;
    *expected = Some(format!("{:x}", Sha256::digest(bytes)));
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    format: String,
    version: u32,
    created_at: String,
    providers: Vec<Value>,
    claude_providers: Vec<Value>,
}

fn portable_provider<T: Serialize>(provider: &T, extra: &[&str]) -> Result<Value, String> {
    let source = serde_json::to_value(provider).map_err(|_| "无法读取供应商配置")?;
    let mut result = Map::new();
    for key in COMMON_FIELDS.iter().chain(extra) {
        if let Some(value) = source.get(*key) {
            result.insert((*key).to_string(), value.clone());
        }
    }
    // Automatic disablement is a local runtime condition, not the user's enable preference.
    result.insert("enabled".into(), json!(source["status"] != "disabled"));
    Ok(Value::Object(result))
}

impl Snapshot {
    fn from_state(state: &ManagerState) -> Result<Self, String> {
        Ok(Self {
            format: FORMAT.into(),
            version: 1,
            created_at: Utc::now().to_rfc3339(),
            providers: state
                .providers
                .iter()
                .map(|p| portable_provider(p, CODEX_FIELDS))
                .collect::<Result<_, _>>()?,
            claude_providers: state
                .claude_providers
                .iter()
                .map(|p| portable_provider(p, CLAUDE_FIELDS))
                .collect::<Result<_, _>>()?,
        })
    }

    fn hash(&self) -> String {
        // Ignore timestamps; key order is deterministic and array order is provider routing order.
        let bytes = serde_json::to_vec(&(&self.providers, &self.claude_providers))
            .expect("JSON values serialize");
        format!("{:x}", Sha256::digest(bytes))
    }

    fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("供应商快照超过 4 MiB 限制".into());
        }
        let snapshot: Self =
            serde_json::from_slice(bytes).map_err(|_| "远端不是有效的 XXSwitch 供应商快照")?;
        if snapshot.format != FORMAT || snapshot.version != 1 {
            return Err("不支持此供应商快照格式或版本，请更新应用".into());
        }
        snapshot.decode()?;
        Ok(snapshot)
    }

    fn decode(&self) -> Result<(Vec<ProviderConfig>, Vec<ClaudeProviderConfig>), String> {
        fn validate(
            value: &Value,
            extra: &[&str],
            ids: &mut BTreeSet<String>,
        ) -> Result<Value, String> {
            let object = value.as_object().ok_or("供应商配置必须是对象")?;
            if object
                .keys()
                .any(|key| !COMMON_FIELDS.contains(&key.as_str()) && !extra.contains(&key.as_str()))
            {
                return Err("快照含有不支持的供应商字段".into());
            }
            let id = value["id"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("供应商 ID 不能为空")?;
            if !ids.insert(id.to_string()) {
                return Err("快照存在重复的供应商 ID".into());
            }
            if value["name"].as_str().is_none_or(|s| s.trim().is_empty()) {
                return Err("供应商名称不能为空".into());
            }
            let enabled = value["enabled"].as_bool().ok_or("供应商启停状态无效")?;
            let mut value = value.clone();
            value["status"] = json!(if enabled { "enabled" } else { "disabled" });
            Ok(value)
        }
        let mut codex_ids = BTreeSet::new();
        let mut claude_ids = BTreeSet::new();
        let providers = self
            .providers
            .iter()
            .map(|v| {
                let v = validate(v, CODEX_FIELDS, &mut codex_ids)?;
                if !v["config"].is_object() {
                    return Err("供应商 TOML 配置无效".into());
                }
                let p: ProviderConfig =
                    serde_json::from_value(v).map_err(|_| "Codex 供应商字段无效")?;
                json_to_toml_text(&p.config).map_err(|_| "供应商 TOML 配置无效")?;
                if p.fusion_enabled && p.allowed_models.is_empty() {
                    return Err("融合供应商缺少启用模型".into());
                }
                Ok(p)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let claude = self
            .claude_providers
            .iter()
            .map(|v| {
                let p: ClaudeProviderConfig =
                    serde_json::from_value(validate(v, CLAUDE_FIELDS, &mut claude_ids)?)
                        .map_err(|_| "Claude 供应商字段无效")?;
                if p.fusion_enabled && p.allowed_models.is_empty() {
                    return Err("融合供应商缺少启用模型".into());
                }
                Ok(p)
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok((providers, claude))
    }

    fn apply(&self, state: &mut ManagerState) -> Result<(), String> {
        let (mut codex, mut claude) = self.decode()?;
        // Keep this device's runtime observations for unchanged providers only.
        for next in &mut codex {
            if let Some(old) = state.providers.iter().find(|p| p.id == next.id) {
                if portable_provider(old, CODEX_FIELDS)? == portable_provider(next, CODEX_FIELDS)? {
                    *next = old.clone();
                }
            }
        }
        for next in &mut claude {
            if let Some(old) = state.claude_providers.iter().find(|p| p.id == next.id) {
                if portable_provider(old, CLAUDE_FIELDS)? == portable_provider(next, CLAUDE_FIELDS)?
                {
                    *next = old.clone();
                }
            }
        }
        state.providers = codex;
        state.claude_providers = claude;
        Ok(())
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct SyncConfig {
    url: String,
    username: String,
    password: String,
    directory: String,
    auto_upload: bool,
    last_local_hash: Option<String>,
    last_remote_hash: Option<String>,
    last_sync_at: Option<String>,
    message: String,
    error: bool,
}

#[derive(Serialize)]
pub(crate) struct SyncView {
    url: String,
    username: String,
    directory: String,
    password_set: bool,
    auto_upload: bool,
    last_sync_at: Option<String>,
    message: String,
    error: bool,
}

impl SyncConfig {
    fn view(&self) -> SyncView {
        SyncView {
            url: self.url.clone(),
            username: self.username.clone(),
            directory: self.directory.clone(),
            password_set: !self.password.is_empty(),
            auto_upload: self.auto_upload,
            last_sync_at: self.last_sync_at.clone(),
            message: self.message.clone(),
            error: self.error,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct SyncDraft {
    url: String,
    username: String,
    // None preserves the saved credential; an explicit empty string clears it.
    password: Option<String>,
    directory: String,
    auto_upload: bool,
}

fn config_path() -> Result<PathBuf, String> {
    Ok(manager_dir()?.join("provider-sync.json"))
}

fn read_config() -> Result<SyncConfig, String> {
    match fs::read(config_path()?) {
        Ok(raw) => serde_json::from_slice(&raw).map_err(|_| "云同步设置文件损坏".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SyncConfig {
            directory: "xxswitch-sync".into(),
            ..Default::default()
        }),
        Err(_) => Err("无法读取云同步设置".into()),
    }
}

fn write_config(config: &SyncConfig) -> Result<(), String> {
    atomic_write(
        &config_path()?,
        &serde_json::to_vec_pretty(config).map_err(|_| "无法保存云同步设置")?,
    )
}

// Replace whole files so the background reader never sees a partially written JSON document.
pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("无法定位配置目录")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建配置目录")?;
    let temp = parent.join(format!(
        ".sync-{}-{}.tmp",
        std::process::id(),
        FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temp).map_err(|_| "无法创建配置临时文件")?;
        file.write_all(bytes).map_err(|_| "无法写入配置文件")?;
        file.sync_all().map_err(|_| "无法同步配置文件")?;
        drop(file);
        fs::rename(&temp, path).map_err(|_| "无法替换配置文件")
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result.map_err(str::to_string)
}

fn read_local() -> Result<ManagerState, String> {
    // Do not run migrations or normalization from the background upload worker.
    let raw = fs::read(state_path()?).map_err(|_| "无法读取本机供应商")?;
    let mut state: ManagerState = serde_json::from_slice(&raw).map_err(|_| "本机供应商配置无效")?;
    state.revision = StateRevision::from_bytes(&raw);
    Ok(state)
}

struct WebDav {
    client: reqwest::Client,
    config: SyncConfig,
    directories: Vec<reqwest::Url>,
    file: reqwest::Url,
}

fn target_urls(config: &SyncConfig) -> Result<(Vec<reqwest::Url>, reqwest::Url), String> {
    let mut url = reqwest::Url::parse(config.url.trim()).map_err(|_| "WebDAV 地址无效")?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("请输入不含账号、密码、查询参数或片段的 HTTP(S) WebDAV 地址".into());
    }
    let mut directories = Vec::new();
    let dir = config.directory.trim().trim_matches('/');
    if dir.is_empty() {
        return Err("远程目录不能为空".into());
    }
    for segment in dir.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(['\\', '\0'])
        {
            return Err("远程目录不能包含空路径、点路径或反斜杠".into());
        }
        url.path_segments_mut()
            .map_err(|_| "WebDAV 地址无效")?
            .pop_if_empty()
            .push(segment)
            .push("");
        directories.push(url.clone());
    }
    url.path_segments_mut()
        .map_err(|_| "WebDAV 地址无效")?
        .pop_if_empty()
        .push("providers-v1.json");
    Ok((directories, url))
}

struct Remote {
    snapshot: Snapshot,
    etag: Option<String>,
}

impl WebDav {
    fn new(config: &SyncConfig) -> Result<Self, String> {
        let (directories, file) = target_urls(config)?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "无法创建 WebDAV 客户端")?;
        Ok(Self {
            client,
            config: config.clone(),
            directories,
            file,
        })
    }

    fn request(&self, method: reqwest::Method, url: reqwest::Url) -> reqwest::RequestBuilder {
        self.client
            .request(method, url)
            .basic_auth(&self.config.username, Some(&self.config.password))
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
        // Never forward reqwest errors or server bodies: they may contain credentials or keys.
        request
            .send()
            .await
            .map_err(|_| "WebDAV 请求失败，请检查地址、网络及证书")
            .map_err(str::to_string)
    }

    async fn get(&self) -> Result<Option<Remote>, String> {
        let mut response = self
            .send(self.request(reqwest::Method::GET, self.file.clone()))
            .await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        check_status(response.status())?;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BYTES as u64)
        {
            return Err("远端供应商快照过大".into());
        }
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.starts_with('"') && v.ends_with('"'))
            .map(str::to_string);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "读取 WebDAV 快照失败")? {
            if bytes.len() + chunk.len() > MAX_BYTES {
                return Err("远端供应商快照过大".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Some(Remote {
            snapshot: Snapshot::parse(&bytes)?,
            etag,
        }))
    }

    async fn mkdir(&self) -> Result<(), String> {
        for url in &self.directories {
            let response = self
                .send(self.request(
                    reqwest::Method::from_bytes(b"MKCOL").expect("valid method"),
                    url.clone(),
                ))
                .await?;
            // RFC 4918 uses 405 for an already existing collection. PUT verifies writability.
            if response.status() != reqwest::StatusCode::METHOD_NOT_ALLOWED {
                check_status(response.status())?;
            }
        }
        Ok(())
    }

    async fn put(&self, snapshot: &Snapshot, remote: Option<&Remote>) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(snapshot).map_err(|_| "无法生成供应商快照")?;
        if bytes.len() > MAX_BYTES {
            return Err("供应商快照超过 4 MiB 限制".into());
        }
        let mut request = self
            .request(reqwest::Method::PUT, self.file.clone())
            .header("Content-Type", "application/json")
            .body(bytes);
        if let Some(remote) = remote {
            let etag = remote
                .etag
                .as_ref()
                .ok_or("此 WebDAV 服务未提供强 ETag，无法安全覆盖远端文件")?;
            request = request.header(IF_MATCH, etag);
        } else {
            request = request.header(IF_NONE_MATCH, "*");
        }
        self.mkdir().await?;
        let response = self.send(request).await?;
        check_status(response.status())
    }
}

fn check_status(status: reqwest::StatusCode) -> Result<(), String> {
    if status.is_success() {
        return Ok(());
    }
    Err(match status.as_u16() {
        401 | 403 => "WebDAV 认证失败或没有访问权限".into(),
        409 => "WebDAV 目录不存在，请检查服务器地址和远程目录".into(),
        412 => "远端已被其他设备修改，请重新预览后选择上传或下载".into(),
        301 | 302 | 307 | 308 => "WebDAV 地址发生重定向，请填写最终服务地址".into(),
        n => format!("WebDAV 返回 HTTP {n}"),
    })
}

#[derive(Serialize)]
pub(crate) struct PreviewView {
    token: String,
    remote_exists: bool,
    local_codex: usize,
    local_claude: usize,
    remote_codex: usize,
    remote_claude: usize,
    remote_created_at: Option<String>,
    same: bool,
}

struct PendingPreview {
    token: String,
    local_hash: String,
    remote_hash: Option<String>,
    created: Instant,
}

fn check_preview(token: &str, local: &Snapshot, remote: Option<&Remote>) -> Result<(), String> {
    let guard = PREVIEW.lock().map_err(|_| "同步预览暂不可用")?;
    let pending = guard
        .as_ref()
        .filter(|p| p.token == token && p.created.elapsed() < Duration::from_secs(300))
        .ok_or("预览已失效，请重新预览")?;
    if local.hash() != pending.local_hash
        || remote.map(|r| r.snapshot.hash()) != pending.remote_hash
    {
        return Err("本机或远端供应商已变化，请重新预览".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn cancel_provider_sync_preview(token: String) {
    if let Ok(mut pending) = PREVIEW.lock() {
        if pending.as_ref().is_some_and(|p| p.token == token) {
            *pending = None;
        }
    }
}

fn record_success(
    config: &mut SyncConfig,
    hash: String,
    message: String,
) -> Result<SyncView, String> {
    config.last_local_hash = Some(hash.clone());
    config.last_remote_hash = Some(hash);
    config.last_sync_at = Some(Utc::now().to_rfc3339());
    config.message = message;
    config.error = false;
    write_config(config)?;
    *PREVIEW.lock().map_err(|_| "同步预览暂不可用")? = None;
    Ok(config.view())
}

#[tauri::command]
pub(crate) async fn get_provider_sync() -> Result<SyncView, String> {
    // Reads are atomic and need not wait for an in-flight network operation.
    Ok(read_config()?.view())
}

#[tauri::command]
pub(crate) async fn save_provider_sync(draft: SyncDraft) -> Result<SyncView, String> {
    let _lock = SYNC_LOCK.lock().await;
    let mut config = read_config()?;
    let changed = config.url != draft.url.trim()
        || config.username != draft.username.trim()
        || config.directory != draft.directory.trim().trim_matches('/');
    config.url = draft.url.trim().to_string();
    config.username = draft.username.trim().to_string();
    config.directory = draft.directory.trim().trim_matches('/').to_string();
    if let Some(password) = draft.password {
        config.password = password;
    }
    config.auto_upload = draft.auto_upload;
    target_urls(&config)?;
    if changed {
        config.last_local_hash = None;
        config.last_remote_hash = None;
        config.last_sync_at = None;
    }
    config.message = "设置已保存".into();
    config.error = false;
    write_config(&config)?;
    *PREVIEW.lock().map_err(|_| "同步预览暂不可用")? = None;
    Ok(config.view())
}

#[tauri::command]
pub(crate) async fn test_provider_sync() -> Result<String, String> {
    let _lock = SYNC_LOCK.lock().await;
    let dav = WebDav::new(&read_config()?)?;
    // PROPFIND verifies authentication without uploading provider keys or creating files.
    let mut url = reqwest::Url::parse(&dav.config.url).map_err(|_| "WebDAV 地址无效")?;
    url.path_segments_mut()
        .map_err(|_| "WebDAV 地址无效")?
        .pop_if_empty()
        .push("");
    let response = dav
        .send(
            dav.request(
                reqwest::Method::from_bytes(b"PROPFIND").expect("valid method"),
                url,
            )
            .header("Depth", "0"),
        )
        .await?;
    check_status(response.status())?;
    Ok("连接成功；上传时将检查目录写入权限".into())
}

#[tauri::command]
pub(crate) async fn preview_provider_sync() -> Result<PreviewView, String> {
    let _lock = SYNC_LOCK.lock().await;
    let remote = WebDav::new(&read_config()?)?.get().await?;
    let local = Snapshot::from_state(&read_local()?)?;
    let token = random_router_token();
    let remote_hash = remote.as_ref().map(|r| r.snapshot.hash());
    let view = PreviewView {
        token: token.clone(),
        remote_exists: remote.is_some(),
        local_codex: local.providers.len(),
        local_claude: local.claude_providers.len(),
        remote_codex: remote.as_ref().map_or(0, |r| r.snapshot.providers.len()),
        remote_claude: remote
            .as_ref()
            .map_or(0, |r| r.snapshot.claude_providers.len()),
        remote_created_at: remote.as_ref().map(|r| r.snapshot.created_at.clone()),
        same: remote_hash.as_ref() == Some(&local.hash()),
    };
    *PREVIEW.lock().map_err(|_| "同步预览暂不可用")? = Some(PendingPreview {
        token,
        local_hash: local.hash(),
        remote_hash,
        created: Instant::now(),
    });
    Ok(view)
}

#[tauri::command]
pub(crate) async fn upload_provider_sync(token: String) -> Result<SyncView, String> {
    let _lock = SYNC_LOCK.lock().await;
    let mut config = read_config()?;
    let dav = WebDav::new(&config)?;
    let remote = dav.get().await?;
    let local = Snapshot::from_state(&read_local()?)?;
    check_preview(&token, &local, remote.as_ref())?;
    if remote
        .as_ref()
        .is_none_or(|r| r.snapshot.hash() != local.hash())
    {
        dav.put(&local, remote.as_ref()).await?;
    }
    record_success(&mut config, local.hash(), "供应商配置已上传".into())
}

#[tauri::command]
pub(crate) async fn download_provider_sync(token: String) -> Result<SyncView, String> {
    let _lock = SYNC_LOCK.lock().await;
    let mut config = read_config()?;
    let remote = WebDav::new(&config)?
        .get()
        .await?
        .ok_or("远端没有供应商快照")?;
    let mut state = read_local()?;
    let local = Snapshot::from_state(&state)?;
    check_preview(&token, &local, Some(&remote))?;
    let mut message = "本机供应商已与远端一致".to_string();
    if local.hash() != remote.snapshot.hash() {
        remote.snapshot.apply(&mut state)?;
        let backup = manager_dir()?.join("provider-sync-backups").join(format!(
            "providers-{}-{}.json",
            Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
            FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        atomic_write(
            &backup,
            &serde_json::to_vec_pretty(&local).map_err(|_| "无法创建供应商备份")?,
        )?;
        save_state(&state)?;
        message = format!("供应商配置已下载；覆盖前备份：{}", backup.display());
    }
    record_success(&mut config, remote.snapshot.hash(), message)
}

fn upload_allowed(config: &SyncConfig, remote: Option<&Remote>) -> bool {
    remote.map(|r| r.snapshot.hash()) == config.last_remote_hash
}

pub(super) fn start_auto_upload() {
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let Ok(_lock) = SYNC_LOCK.try_lock() else {
                continue;
            };
            let Ok(mut config) = read_config() else {
                continue;
            };
            if !config.auto_upload || config.url.is_empty() {
                continue;
            }
            // A human preview must remain stable until it is confirmed or expires.
            if PREVIEW
                .lock()
                .map(|p| {
                    p.as_ref()
                        .is_some_and(|p| p.created.elapsed() < Duration::from_secs(300))
                })
                .unwrap_or(true)
            {
                continue;
            }
            let result = auto_upload(&mut config).await;
            if let Err(error) = result {
                if !config.error || config.message != error {
                    config.error = true;
                    config.message = error;
                    let _ = write_config(&config);
                }
            }
        }
    });
}

async fn auto_upload(config: &mut SyncConfig) -> Result<(), String> {
    let local = Snapshot::from_state(&read_local()?)?;
    if config.last_local_hash.as_ref() == Some(&local.hash()) {
        return Ok(());
    }
    let dav = WebDav::new(config)?;
    let remote = dav.get().await?;
    if remote
        .as_ref()
        .is_some_and(|r| r.snapshot.hash() == local.hash())
    {
        record_success(config, local.hash(), "供应商配置已与远端一致".into())?;
        return Ok(());
    }
    if !upload_allowed(config, remote.as_ref()) {
        return Err("远端供应商已变化，自动上传已暂停；请预览并选择上传或下载".into());
    }
    dav.put(&local, remote.as_ref()).await?;
    record_success(config, local.hash(), "供应商配置已自动上传".into())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ManagerState {
        let mut state = default_state();
        let mut p = new_codex_provider("codex-one".into(), "Codex One".into()).unwrap();
        apply_provider_connection_draft(
            &mut p,
            Some("https://example.invalid/v1"),
            Some("fixture-key"),
        )
        .unwrap();
        p.allowed_models = vec!["model-a".into()];
        p.service_tier = "priority".into();
        p.supports_websockets = true;
        p.balance_query.query_token = "fixture-balance-token".into();
        state.providers.push(p);
        let mut c = new_claude_provider("claude-one".into(), "Claude One".into()).unwrap();
        c.api_key = "fixture-claude-key".into();
        c.base_url = "https://claude.example.invalid".into();
        c.status = ProviderStatus::Disabled;
        c.enabled = false;
        state.claude_providers.push(c);
        state
    }

    #[test]
    fn provider_sync_snapshot_excludes_device_and_runtime_data() {
        let mut state = fixture();
        let expected = Snapshot::from_state(&state).unwrap();
        state.router.local_token = "DO-NOT-SYNC-LOCAL-TOKEN".into();
        state.base = json!({"local_only": "DO-NOT-SYNC-BASE"});
        state.last_applied = Some(json!({"secret": "DO-NOT-SYNC-BACKUP"}));
        state.clients.codex.managed_skill_location = "DO-NOT-SYNC-SKILLS".into();
        state.providers[0].status = ProviderStatus::AutoDisabled;
        state.providers[0].enabled = false;
        state.providers[0].consecutive_failure_count = 3;
        state.providers[0].last_failure_reason = Some("DO-NOT-SYNC-FAILURE".into());
        let snapshot = Snapshot::from_state(&state).unwrap();
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains("DO-NOT-SYNC"));
        assert!(!serialized.contains("consecutive_failure_count"));
        assert!(!serialized.contains("connection_status"));
        assert!(!serialized.contains("balance_status"));
        assert_eq!(snapshot.hash(), expected.hash());
        assert_eq!(snapshot.providers[0]["enabled"], true);
        assert_eq!(snapshot.providers[0]["supports_websockets"], true);
        assert_eq!(snapshot.claude_providers[0]["enabled"], false);
        assert!(serialized.contains("fixture-key"));
        assert!(serialized.contains("fixture-balance-token"));
        let roundtrip = Snapshot::parse(serialized.as_bytes()).unwrap();
        assert_eq!(roundtrip.hash(), snapshot.hash());
    }

    #[test]
    fn provider_sync_download_changes_only_supplier_lists() {
        let mut target = fixture();
        target.router.local_token = "local-token".into();
        target.router.port = 19191;
        target.clients.codex.managed_skill_location = "/local/path".into();
        target.last_applied = Some(json!({"local": true}));
        target.providers[0].consecutive_failure_count = 2;
        let before = serde_json::to_value(&target).unwrap();
        let mut source = fixture();
        source.claude_providers[0].name = "Remote Claude".into();
        source
            .providers
            .push(new_codex_provider("second".into(), "Second".into()).unwrap());
        source.providers.swap(0, 1);
        let snapshot = Snapshot::from_state(&source).unwrap();
        snapshot.apply(&mut target).unwrap();
        let after = serde_json::to_value(&target).unwrap();
        for (key, value) in before.as_object().unwrap() {
            if key != "providers" && key != "claude_providers" {
                assert_eq!(&after[key], value, "changed {key}");
            }
        }
        assert_eq!(target.providers[0].id, "second");
        assert_eq!(target.providers[1].consecutive_failure_count, 2);
        assert_eq!(target.claude_providers[0].name, "Remote Claude");
        assert_eq!(
            Snapshot::from_state(&target).unwrap().hash(),
            snapshot.hash()
        );
        let before_invalid = serde_json::to_value(&target).unwrap();
        let mut invalid = snapshot.clone();
        invalid.claude_providers[0]["api_key"] = json!(42);
        assert!(invalid.apply(&mut target).is_err());
        assert_eq!(serde_json::to_value(&target).unwrap(), before_invalid);
    }

    #[test]
    fn provider_sync_rejects_wrong_versions_duplicates_and_runtime_fields() {
        let original = Snapshot::from_state(&fixture()).unwrap();
        let mut invalid = original.clone();
        invalid.version = 2;
        assert!(Snapshot::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
        invalid = original.clone();
        invalid.providers.push(invalid.providers[0].clone());
        assert!(invalid.decode().is_err());
        invalid = original.clone();
        invalid.providers[0]["last_failure_reason"] = json!("remote-runtime");
        assert!(invalid.decode().is_err());
        invalid = original;
        invalid.providers[0]["fusion_enabled"] = json!(true);
        invalid.providers[0]["allowed_models"] = json!([]);
        assert!(invalid.decode().is_err());
        assert!(Snapshot::parse(&vec![b' '; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn provider_sync_paths_stay_inside_webdav_root() {
        let mut config = SyncConfig {
            url: "https://example.invalid/remote.php/dav/files/user".into(),
            directory: "共享/供应商 #1".into(),
            ..Default::default()
        };
        let (dirs, file) = target_urls(&config).unwrap();
        assert_eq!(dirs.len(), 2);
        assert!(file
            .as_str()
            .starts_with("https://example.invalid/remote.php/dav/files/user/"));
        assert!(file.as_str().contains("%231/providers-v1.json"));
        for directory in ["", "../escape", "a/../b", "a//b", "a\\b"] {
            config.directory = directory.into();
            assert!(target_urls(&config).is_err(), "accepted {directory}");
        }
        config.directory = "valid".into();
        for url in [
            "file:///tmp",
            "https://user:secret@example.invalid/",
            "https://example.invalid/?token=secret",
            "https://example.invalid/#secret",
        ] {
            config.url = url.into();
            assert!(target_urls(&config).is_err());
        }
    }

    #[test]
    fn provider_sync_conflict_policy_requires_known_remote() {
        let remote = Remote {
            snapshot: Snapshot::from_state(&fixture()).unwrap(),
            etag: Some("\"1\"".into()),
        };
        let mut config = SyncConfig::default();
        assert!(upload_allowed(&config, None));
        assert!(!upload_allowed(&config, Some(&remote)));
        config.last_remote_hash = Some(remote.snapshot.hash());
        assert!(upload_allowed(&config, Some(&remote)));
        assert!(!upload_allowed(&config, None));
        let mut changed = remote.snapshot.clone();
        changed.providers[0]["name"] = json!("changed elsewhere");
        assert!(!upload_allowed(
            &config,
            Some(&Remote {
                snapshot: changed,
                etag: None
            })
        ));
    }

    #[test]
    fn provider_sync_preview_rejects_changed_local_remote_and_expiry() {
        let local = Snapshot::from_state(&fixture()).unwrap();
        let mut remote = Remote {
            snapshot: local.clone(),
            etag: None,
        };
        *PREVIEW.lock().unwrap() = Some(PendingPreview {
            token: "test-preview".into(),
            local_hash: local.hash(),
            remote_hash: Some(remote.snapshot.hash()),
            created: Instant::now(),
        });
        assert!(check_preview("wrong-token", &local, Some(&remote)).is_err());
        assert!(check_preview("test-preview", &local, Some(&remote)).is_ok());
        let mut changed = local.clone();
        changed.providers[0]["name"] = json!("changed locally");
        assert!(check_preview("test-preview", &changed, Some(&remote)).is_err());
        remote.snapshot.providers[0]["name"] = json!("changed remotely");
        assert!(check_preview("test-preview", &local, Some(&remote)).is_err());
        assert!(check_preview("test-preview", &local, None).is_err());
        remote.snapshot = local.clone();
        PREVIEW.lock().unwrap().as_mut().unwrap().created =
            Instant::now() - Duration::from_secs(301);
        assert!(check_preview("test-preview", &local, Some(&remote)).is_err());
        cancel_provider_sync_preview("test-preview".into());
        assert!(PREVIEW.lock().unwrap().is_none());
    }

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "xxswitch-provider-sync-{}-{}",
                std::process::id(),
                random_router_token()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn provider_sync_atomic_writes_reject_stale_state_and_preserve_backup() {
        let dir = TestDir::new();
        let path = dir.0.join("state.json");
        let initial = StateRevision::default();
        write_state_checked(&path, b"old-state", &initial).unwrap();
        let stale = initial.clone();
        atomic_write(&dir.0.join("backup.json"), b"old-state").unwrap();
        write_state_checked(&path, b"new-state", &initial).unwrap();
        assert!(write_state_checked(&path, b"stale-state", &stale).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"new-state");
        assert_eq!(fs::read(dir.0.join("backup.json")).unwrap(), b"old-state");
        write_state_checked(&path, b"newer-state", &initial).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
    }

    #[derive(Default)]
    struct MockDav {
        body: Option<Vec<u8>>,
        generation: u32,
        omit_etag: bool,
        puts: usize,
    }

    async fn serve_dav(
        AxumState(state): AxumState<Arc<Mutex<MockDav>>>,
        request: axum::extract::Request,
    ) -> Response {
        if request.headers().get(AUTHORIZATION).is_none() {
            return Response::builder()
                .status(401)
                .body(Body::from("secret-server-error"))
                .unwrap();
        }
        let method = request.method().clone();
        let headers = request.headers().clone();
        let bytes = axum::body::to_bytes(request.into_body(), MAX_BYTES + 1)
            .await
            .unwrap();
        let mut state = state.lock().unwrap();
        match method.as_str() {
            "PROPFIND" => Response::builder().status(207).body(Body::empty()).unwrap(),
            "MKCOL" => Response::builder().status(405).body(Body::empty()).unwrap(),
            "GET" => match &state.body {
                Some(body) => {
                    let mut response = Response::builder().status(200);
                    if !state.omit_etag {
                        response = response.header(ETAG, format!("\"{}\"", state.generation));
                    }
                    response.body(Body::from(body.clone())).unwrap()
                }
                None => Response::builder().status(404).body(Body::empty()).unwrap(),
            },
            "PUT" => {
                let matches = if state.body.is_some() {
                    headers.get(IF_MATCH).and_then(|v| v.to_str().ok())
                        == Some(format!("\"{}\"", state.generation).as_str())
                } else {
                    headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some("*")
                };
                if !matches {
                    return Response::builder().status(412).body(Body::empty()).unwrap();
                }
                state.body = Some(bytes.to_vec());
                state.generation += 1;
                state.puts += 1;
                Response::builder().status(201).body(Body::empty()).unwrap()
            }
            _ => Response::builder().status(405).body(Body::empty()).unwrap(),
        }
    }

    #[tokio::test]
    async fn provider_sync_webdav_roundtrip_and_conditional_conflict() {
        let state = Arc::new(Mutex::new(MockDav::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().fallback(serve_dav).with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = SyncConfig {
            url: format!("http://{address}/dav/"),
            directory: "nested/providers".into(),
            username: "test".into(),
            password: "test-password".into(),
            ..Default::default()
        };
        let mut dav = WebDav::new(&config).unwrap();
        // Tests must not depend on the host's system proxy.
        dav.client = reqwest::Client::builder().no_proxy().build().unwrap();
        let original = Snapshot::from_state(&fixture()).unwrap();
        assert!(dav.get().await.unwrap().is_none());
        dav.put(&original, None).await.unwrap();
        let remote = dav.get().await.unwrap().unwrap();
        assert_eq!(remote.snapshot.hash(), original.hash());
        assert!(dav
            .put(&original, None)
            .await
            .unwrap_err()
            .contains("其他设备"));
        let mut changed = original.clone();
        changed.providers[0]["name"] = json!("renamed");
        dav.put(&changed, Some(&remote)).await.unwrap();
        assert!(dav
            .put(&original, Some(&remote))
            .await
            .unwrap_err()
            .contains("其他设备"));
        assert_eq!(
            dav.get().await.unwrap().unwrap().snapshot.hash(),
            changed.hash()
        );
        state.lock().unwrap().omit_etag = true;
        let remote = dav.get().await.unwrap().unwrap();
        assert!(dav
            .put(&original, Some(&remote))
            .await
            .unwrap_err()
            .contains("ETag"));
        assert_eq!(state.lock().unwrap().puts, 2);
        state.lock().unwrap().body = Some(b"not a snapshot: secret-key".to_vec());
        let error = dav.get().await.err().unwrap();
        assert!(!error.contains("secret-key"));
        server.abort();
    }
}
