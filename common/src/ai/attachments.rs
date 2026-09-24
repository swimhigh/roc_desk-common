//! 多轮 Agent 对话的附件（图片/文本文件/PDF）处理——原本只在宿主
//! `coding::session` 里实现，SQL Agent（`roc_desk_sql`）需要一份完全相同的
//! 逻辑却因为它是 host-only 代码没法直接依赖，因此和 `agent_llm`/`ai` 一起
//! 挪到这个共享 crate。`event_prefix` 参数取代原来硬编码的 `"coding:"`
//! 事件名前缀，让不同 Agent（`"coding"`/`"sqlagent"`）复用同一套实现但各自
//! 发到自己的前端事件通道上。

use base64::Engine;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::agent_llm::{context_budget, estimate_tokens};

use super::providers::AiProvider;

/// 本地文件读成 base64/文本再传过来——后端不碰用户的本地文件系统，天然对齐
/// "远程工作区也能用附件"（附件来自用户本机，不是工作区所在的主机）。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatAttachment {
    Image {
        name: String,
        mime: String,
        data_base64: String,
    },
    File {
        name: String,
        content: String,
    },
    /// PDF——前端不解析内容，只把原始文件读成 base64 传过来（和 `Image` 一样
    /// "后端不碰用户本机文件系统"），真正的文本抽取用 `pdf_extract` 做。
    Pdf {
        name: String,
        data_base64: String,
    },
}

/// 把用户这句话 + 附件合成成最终发给模型的 `content`（纯字符串，或者带图片
/// 的多模态 parts 数组）。附件太大时自动分窗口用一次轻量 LLM 调用提取"和这次
/// 问题相关的部分"代替全文，预算算法和 `agent_llm::context_budget` 保持一致。
#[allow(clippy::too_many_arguments)]
pub async fn build_user_message_content(
    user_text: &str,
    attachments: &[ChatAttachment],
    client: &reqwest::Client,
    provider: &AiProvider,
    api_key: &Option<String>,
    app_handle: &AppHandle,
    session_id: Uuid,
    event_prefix: &str,
) -> serde_json::Value {
    if attachments.is_empty() {
        return json!(user_text);
    }

    struct ResolvedAttachment {
        label: String,
        text: String,
    }
    let mut resolved: Vec<ResolvedAttachment> = Vec::new();
    for attachment in attachments {
        match attachment {
            ChatAttachment::File { name, content } => {
                resolved.push(ResolvedAttachment { label: name.clone(), text: content.clone() });
            }
            ChatAttachment::Pdf { name, data_base64 } => {
                let text = extract_pdf_text_raw(data_base64)
                    .unwrap_or_else(|e| format!("[PDF 「{name}」解析失败：{e}——可能是加密/损坏/格式不受支持的 PDF]"));
                resolved.push(ResolvedAttachment { label: name.clone(), text });
            }
            ChatAttachment::Image { .. } => {}
        }
    }

    // 预算算法和 `agent_llm::context_budget` 保持一致（同一个数字）；只给附件
    // 本身留一半预算——剩下的要留给 system 提示词/工具 schema/这句话本身/
    // 模型的回答空间，全部吃满反而更容易在别的地方再触发一次同一个预算保护。
    let budget = context_budget(provider);
    let attachment_budget = budget / 2;
    let total_estimate: usize = resolved.iter().map(|r| estimate_tokens(r.text.len())).sum();
    let fair_share = attachment_budget / resolved.len().max(1);

    let mut text = user_text.to_string();
    if total_estimate <= attachment_budget {
        for r in &resolved {
            text.push_str(&format!("\n\n--- 附件文件: {} ---\n{}", r.label, r.text));
        }
    } else {
        for r in &resolved {
            let contribution = if estimate_tokens(r.text.len()) > fair_share {
                condense_attachment_text(client, provider, api_key, user_text, &r.label, &r.text, app_handle, session_id, event_prefix)
                    .await
            } else {
                r.text.clone()
            };
            text.push_str(&format!("\n\n--- 附件文件: {} ---\n{}", r.label, contribution));
        }
    }

    let mut parts = vec![json!({ "type": "text", "text": text })];
    for attachment in attachments {
        if let ChatAttachment::Image {
            mime, data_base64, ..
        } = attachment
        {
            parts.push(json!({
                "type": "image_url",
                "image_url": { "url": format!("data:{mime};base64,{data_base64}") }
            }));
        }
    }
    serde_json::Value::Array(parts)
}

/// 原始提取文本超过这个字符数就先硬截断再分窗口——防止一份异常巨大的 PDF
/// （几十万字，比如整本扫描书籍的 OCR 文字层）被切成成百上千个窗口、打出
/// 成百上千次 LLM 请求，那不是"自动分窗口处理"想要的效果，是另一种失控。
/// 200 万字符（约 66 万 token 估算）留了足够大的余量给正常的大文档。
const MAX_PDF_RAW_CHARS: usize = 2_000_000;

/// 解码 base64 + 用 `pdf_extract` 抽取纯文本，不做任何截断/预算判断——那是
/// `build_user_message_content` 的职责，这里只负责"这份 PDF 里到底写了什么字"。
fn extract_pdf_text_raw(data_base64: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64)
        .map_err(|e| format!("附件解码失败：{e}"))?;
    let text = pdf_extract::extract_text_from_mem(&bytes).map_err(|e| e.to_string())?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("没有可提取的文本内容——可能是纯扫描图片版，没有文字层".to_string());
    }
    if trimmed.chars().count() > MAX_PDF_RAW_CHARS {
        let truncated: String = trimmed.chars().take(MAX_PDF_RAW_CHARS).collect();
        return Ok(format!("{truncated}\n...[原文档异常巨大，已先截断到前 {MAX_PDF_RAW_CHARS} 字符，超出部分没有参与后续处理]"));
    }
    Ok(trimmed.to_string())
}

/// 单个"窗口"的目标 token 数——不是硬上限，是"这一个窗口大概能安全用掉多少
/// 预算"的粗略目标。
const ATTACHMENT_WINDOW_TARGET_TOKENS: usize = 6_000;
/// 单个窗口提取请求的超时——超时/失败就把这个窗口原文（截断一部分）直接保留，
/// 不阻塞整个流程。
const ATTACHMENT_WINDOW_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 按行边界把一段长文本切成若干个目标大小在 `target_tokens` 附近的窗口。
fn split_into_windows(text: &str, target_tokens: usize) -> Vec<String> {
    let target_chars = target_tokens.saturating_mul(3).max(1);
    if text.chars().count() <= target_chars {
        return vec![text.to_string()];
    }
    let mut windows = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0usize;
    for line in text.split_inclusive('\n') {
        let line_chars = line.chars().count();
        if current_chars + line_chars > target_chars && !current.is_empty() {
            windows.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        current.push_str(line);
        current_chars += line_chars;
    }
    if !current.is_empty() {
        windows.push(current);
    }
    windows
}

/// 附件内容太大、直接塞进消息正文会让请求超出 Provider 的上下文预算时，自动
/// 按窗口拆分、逐窗口用一次轻量 LLM 调用提取"和用户这次问题相关的内容"，再把
/// 所有窗口的提取结果拼起来代替原始全文。故意不用 `agent_llm::call_llm_once`
/// （那一套完整的双协议归一化/429 重试机制）——这是主流程之外的辅助步骤，
/// 摘要/提取本身失败不能变成新的卡死点，宁可退化成"保留原文的一部分"也不要
/// 因为这一步反复重试拖住整个请求。
///
/// 每个窗口互相看不到彼此的内容——窗口之间没有"记忆"，这是简化设计的代价：
/// 真正需要完整看一遍全文做统计类任务的场景，这个机制帮不上忙。
#[allow(clippy::too_many_arguments)]
async fn condense_attachment_text(
    client: &reqwest::Client,
    provider: &AiProvider,
    api_key: &Option<String>,
    user_text: &str,
    attachment_name: &str,
    full_text: &str,
    app_handle: &AppHandle,
    session_id: Uuid,
    event_prefix: &str,
) -> String {
    let windows = split_into_windows(full_text, ATTACHMENT_WINDOW_TARGET_TOKENS);
    if windows.len() <= 1 {
        return full_text.to_string();
    }
    let _ = app_handle.emit(
        &format!("{event_prefix}:assistant-note"),
        json!({
            "sessionId": session_id,
            "text": format!(
                "附件「{attachment_name}」内容较多（约 {} 字），超出了当前 Provider 的上下文预算，\
                 已自动拆成 {} 个窗口分别提取与你的问题相关的部分…",
                full_text.chars().count(),
                windows.len()
            ),
            "kind": "status"
        }),
    );
    let window_count = windows.len();
    let mut parts: Vec<String> = Vec::with_capacity(window_count);
    for (i, window) in windows.iter().enumerate() {
        match summarize_attachment_window(client, provider, api_key, user_text, attachment_name, i + 1, window_count, window)
            .await
        {
            Some(extracted) if extracted.contains("此部分与问题无关") => {}
            Some(extracted) => parts.push(format!("[第 {}/{window_count} 部分]\n{extracted}", i + 1)),
            None => parts.push(format!(
                "[第 {}/{window_count} 部分：自动提取超时/失败，保留原文前 2000 字符]\n{}",
                i + 1,
                window.chars().take(2000).collect::<String>()
            )),
        }
    }
    if parts.is_empty() {
        return format!(
            "（附件《{attachment_name}》内容较多，已自动分成 {window_count} 个窗口检查，但没有找到和\
             当前问题明显相关的内容——如果确定文档里有相关信息，换一个更具体的问题描述再试，或者直接\
             告诉我大概在文档的哪个部分）"
        );
    }
    format!(
        "（原文档较长，超出了当前上下文预算，已自动拆成 {window_count} 个窗口、分别提取与你的问题\
         「{user_text}」相关的内容，以下是各窗口提取结果的合并，不是原文全文）\n\n{}",
        parts.join("\n\n")
    )
}

/// `condense_attachment_text` 的单个窗口——假设 Provider 是 chat/completions
/// 协议（这个 codebase 里"锦上添花"的辅助 LLM 调用目前都是这个简化，暂不支持
/// Responses-only 协议的 Provider 走这条路径——那种 Provider 会退化成
/// "保留原文片段"而不是提取失败崩溃）。
async fn summarize_attachment_window(
    client: &reqwest::Client,
    provider: &AiProvider,
    api_key: &Option<String>,
    user_text: &str,
    attachment_name: &str,
    window_index: usize,
    window_count: usize,
    window_text: &str,
) -> Option<String> {
    let url = format!("{}/chat/completions", provider.api_base.trim_end_matches('/'));
    let body = json!({
        "model": provider.model,
        "messages": [
            {
                "role": "system",
                "content": format!(
                    "你在帮用户从一份过长的附件文档《{attachment_name}》里挑出和他的问题相关的内容——\
                     这份文档太大，已经被自动切成 {window_count} 个窗口分别处理，你现在看到的是第 \
                     {window_index}/{window_count} 部分，看不到其它部分。用户的问题是：『{user_text}』。\
                     请从这部分内容里提取和这个问题直接相关的信息（具体的数据、表名/字段名/接口定义/\
                     结论等，能保留原文措辞就保留，不要过度概括丢细节），无关的内容直接跳过不用提。\
                     如果这部分内容整体上和问题没有关系，只回复\"（此部分与问题无关）\"这一句，不要\
                     硬凑内容。直接输出提取结果，不要复述这段说明、不要说\"好的\"\"以下是\"这类开场白。"
                )
            },
            { "role": "user", "content": window_text }
        ]
    });
    let mut req = client.post(&url).json(&body);
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    let resp = match tokio::time::timeout(ATTACHMENT_WINDOW_TIMEOUT, req.send()).await {
        Ok(Ok(resp)) => resp,
        _ => return None,
    };
    let body: serde_json::Value = match resp.json().await {
        Ok(body) => body,
        Err(_) => return None,
    };
    body["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
