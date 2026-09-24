use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use roc_desk_core::credential::CredentialStore;
use roc_desk_core::error::AppError;

use super::providers_repo::AiProvidersRepo;

/// 多模型适配（DESIGN.md §3.6）：豆包/OpenAI 兼容/DeepSeek/通义千问都走同一套
/// OpenAI 兼容协议，区别只在 `api_base`/`model`；本地 Ollama 同样兼容该协议，
/// 用 `is_local` 标注是为了 UI 做"本地/云端"视觉区分和未来的脱敏策略判断
/// （云端 Provider 才需要担心 DESIGN.md §3.6 提到的数据出境风险）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvider {
    pub id: Uuid,
    pub name: String,
    pub api_base: String,
    pub api_key_ref: Option<String>,
    pub model: String,
    pub is_local: bool,
    /// `"chat_completions"`（默认）或 `"responses"`——2026-09 老引擎新增 OpenAI
    /// Responses API 支持后，每个 provider 要标注自己用哪种 wire 协议，用户在
    /// Provider 设置里手动选，不做自动探测。
    pub wire_api: String,
    /// 对齐 Codex `config.toml` 的 `model_reasoning_effort`——gpt-5/o 系列这类
    /// 推理模型可以从客户端调节内部推理力度。`None`/空字符串表示不传这个参数。
    pub reasoning_effort: Option<String>,
    /// 这个 Provider 实际能接受的上下文窗口（估算 token 数）——各 Agent 用它
    /// 替代写死的保守默认值。
    pub context_window_tokens: Option<u32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiProviderInput {
    pub name: String,
    pub api_base: String,
    pub api_key: Option<String>,
    pub model: String,
    pub is_local: bool,
    pub wire_api: String,
    pub reasoning_effort: Option<String>,
    pub context_window_tokens: Option<u32>,
}

fn credential_key(id: Uuid) -> String {
    format!("ai:{id}:api_key")
}

/// 前端下拉框"不设置"选项传的是空字符串而不是 `null`（HTML `<select>` 的
/// 惯常写法）——这里统一收敛成 `None`，避免数据库里存进一个空字符串。
fn normalize_reasoning_effort(effort: Option<String>) -> Option<String> {
    effort.filter(|e| !e.trim().is_empty())
}

pub struct AiProviderManager {
    repo: Arc<AiProvidersRepo>,
    credential_store: Arc<dyn CredentialStore>,
}

impl AiProviderManager {
    pub fn new(repo: Arc<AiProvidersRepo>, credential_store: Arc<dyn CredentialStore>) -> Self {
        Self {
            repo,
            credential_store,
        }
    }

    pub async fn create(&self, input: AiProviderInput) -> Result<AiProvider, AppError> {
        let id = Uuid::new_v4();
        let api_key_ref = if let Some(key) = &input.api_key {
            if key.is_empty() {
                None
            } else {
                let cred_key = credential_key(id);
                self.credential_store.set(&cred_key, key).await?;
                Some(cred_key)
            }
        } else {
            None
        };

        let provider = AiProvider {
            id,
            name: input.name,
            api_base: input.api_base,
            api_key_ref,
            model: input.model,
            is_local: input.is_local,
            wire_api: input.wire_api,
            reasoning_effort: normalize_reasoning_effort(input.reasoning_effort),
            context_window_tokens: input.context_window_tokens,
            created_at: Utc::now().to_rfc3339(),
        };
        self.repo.create(&provider)?;
        Ok(provider)
    }

    pub async fn update(&self, id: Uuid, input: AiProviderInput) -> Result<AiProvider, AppError> {
        let existing = self
            .repo
            .get(id)?
            .ok_or_else(|| AppError::NotFound(format!("ai provider not found: {id}")))?;

        let api_key_ref = if let Some(key) = &input.api_key {
            if key.is_empty() {
                existing.api_key_ref
            } else {
                let cred_key = existing
                    .api_key_ref
                    .clone()
                    .unwrap_or_else(|| credential_key(id));
                self.credential_store.set(&cred_key, key).await?;
                Some(cred_key)
            }
        } else {
            existing.api_key_ref
        };

        let provider = AiProvider {
            id,
            name: input.name,
            api_base: input.api_base,
            api_key_ref,
            model: input.model,
            is_local: input.is_local,
            wire_api: input.wire_api,
            reasoning_effort: normalize_reasoning_effort(input.reasoning_effort),
            context_window_tokens: input.context_window_tokens,
            created_at: existing.created_at,
        };
        self.repo.update(&provider)?;
        Ok(provider)
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), AppError> {
        if let Some(existing) = self.repo.get(id)? {
            if let Some(key) = existing.api_key_ref {
                self.credential_store.delete(&key).await?;
            }
        }
        self.repo.delete(id)?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<AiProvider>, AppError> {
        self.repo.list()
    }

    pub fn get(&self, id: Uuid) -> Result<Option<AiProvider>, AppError> {
        self.repo.get(id)
    }

    pub async fn resolve_api_key(&self, provider: &AiProvider) -> Result<Option<String>, AppError> {
        match &provider.api_key_ref {
            Some(key) => self.credential_store.get(key).await,
            None => Ok(None),
        }
    }

    /// 拉取这个 Provider 实际支持的模型列表——OpenAI 兼容协议的 `GET /models`。
    /// 不缓存——每次调用都是一次真实请求。
    pub async fn list_models(&self, id: Uuid) -> Result<Vec<String>, AppError> {
        let provider = self
            .get(id)?
            .ok_or_else(|| AppError::NotFound(format!("ai provider not found: {id}")))?;
        let api_key = self.resolve_api_key(&provider).await?;
        let url = format!("{}/models", provider.api_base.trim_end_matches('/'));
        let client = reqwest::Client::new();
        let mut req = client.get(&url);
        if let Some(key) = &api_key {
            req = req.bearer_auth(key);
        }
        let resp = tokio::time::timeout(std::time::Duration::from_secs(10), req.send())
            .await
            .map_err(|_| AppError::Connection("获取模型列表超时".to_string()))??;
        if !resp.status().is_success() {
            return Err(AppError::Connection(format!(
                "获取模型列表失败：HTTP {}",
                resp.status()
            )));
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Connection(format!("解析模型列表响应失败：{e}")))?;
        let mut models: Vec<String> = body["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v["id"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        models.sort();
        models.dedup();
        Ok(models)
    }
}
