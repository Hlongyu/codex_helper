//! Maintainer-owned capabilities. The client only validates, caches and selects rows.
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::Path, time::Duration};

pub const URL: &str = "http://127.0.0.1:18081/catalog.json";
const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub schema_version: u32,
    pub version: String,
    pub openai: Vec<Value>,
    pub deepseek: Vec<Value>,
    pub default: Value,
}

impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("模型能力目录超过 8 MiB".into());
        }
        let catalog: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("目录 JSON 无效: {e}"))?;
        if catalog.schema_version != 1 || catalog.version.trim().is_empty() {
            return Err("目录格式版本或发布版本无效".into());
        }
        if catalog.openai.is_empty() || catalog.deepseek.is_empty() {
            return Err("目录必须同时包含 OpenAI 和 DeepSeek 模型".into());
        }
        let mut ids = BTreeSet::new();
        for row in catalog
            .openai
            .iter()
            .chain(&catalog.deepseek)
            .chain(std::iter::once(&catalog.default))
        {
            let slug = row["slug"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("模型缺少 slug")?;
            if !ids.insert(slug.to_lowercase()) {
                return Err(format!("重复的模型 ID: {slug}"));
            }
            for key in [
                "display_name",
                "base_instructions",
                "shell_type",
                "default_reasoning_level",
            ] {
                if row[key].as_str().is_none() {
                    return Err(format!("{slug} 缺少字符串字段 {key}"));
                }
            }
            if row["context_window"].as_u64().unwrap_or(0) == 0 {
                return Err(format!("{slug} 的上下文窗口无效"));
            }
            for key in ["input_modalities", "supported_reasoning_levels"] {
                if row[key].as_array().is_none_or(Vec::is_empty) {
                    return Err(format!("{slug} 缺少 {key}"));
                }
            }
        }
        Ok(catalog)
    }

    pub fn rows(&self) -> Vec<Value> {
        self.openai.iter().chain(&self.deepseek).cloned().collect()
    }

    pub fn select(&self, model: &str) -> Value {
        let matched = self.openai.iter().chain(&self.deepseek).find(|r| {
            r["slug"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(model))
        });
        let mut row = matched.unwrap_or(&self.default).clone();
        row["slug"] = json!(model);
        if matched.is_none() {
            row["display_name"] = json!(model);
        }
        row
    }
}

pub fn bundled() -> Catalog {
    Catalog::parse(include_bytes!("../../model-catalog/catalog.json"))
        .expect("invalid bundled capability snapshot")
}

pub fn load(path: &Path) -> Result<Catalog, String> {
    match fs::read(path) {
        Ok(bytes) => Catalog::parse(&bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(bundled()),
        Err(e) => Err(format!("读取模型能力缓存失败: {e}")),
    }
}

pub async fn download(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    // Intentionally never attach provider credentials to this public catalog request.
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载模型能力失败: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("目录服务返回 {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err("模型能力目录过大".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err("模型能力目录过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Catalog::parse(&bytes)?;
    Ok(bytes)
}

pub fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    Catalog::parse(bytes)?;
    fs::create_dir_all(path.parent().ok_or("无效缓存路径")?).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    fs::rename(&temporary, path).map_err(|e| format!("替换模型能力缓存失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_remote_capabilities_without_local_inference() {
        let mut catalog = bundled();
        catalog.openai[0]["context_window"] = json!(7654321);
        let slug = catalog.openai[0]["slug"].as_str().unwrap();
        assert_eq!(catalog.select(slug)["context_window"], 7654321);
        catalog.default["context_window"] = json!(23456);
        let unknown = catalog.select("future-model");
        assert_eq!(unknown["context_window"], 23456);
        assert_eq!(unknown["slug"], "future-model");
        assert_eq!(
            unknown["base_instructions"],
            catalog.default["base_instructions"]
        );
    }
    #[test]
    fn invalid_updates_do_not_replace_cache() {
        let dir = std::env::temp_dir().join(format!("xxswitch-catalog-{}", std::process::id()));
        let path = dir.join("catalog.json");
        let bytes = serde_json::to_vec(&bundled()).unwrap();
        save(&path, &bytes).unwrap();
        assert!(save(&path, br#"{"version":"broken"}"#).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mut catalog = bundled();
        catalog.version = "new".into();
        save(&path, &serde_json::to_vec(&catalog).unwrap()).unwrap();
        assert_eq!(load(&path).unwrap().version, "new");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rejects_duplicate_and_incomplete_entries() {
        let mut catalog = bundled();
        catalog.deepseek.push(catalog.openai[0].clone());
        assert!(Catalog::parse(&serde_json::to_vec(&catalog).unwrap()).is_err());
        let mut catalog = bundled();
        catalog
            .default
            .as_object_mut()
            .unwrap()
            .remove("base_instructions");
        assert!(Catalog::parse(&serde_json::to_vec(&catalog).unwrap()).is_err());
    }
}
