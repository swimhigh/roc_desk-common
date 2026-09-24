//! `todo_write` 工具的数据形状——多轮 Agent 用它维护一份结构化任务清单向前端
//! 展示实时进度。两个 Agent（编程助手/SQL Agent）的 `todo_write` 工具定义和
//! 语义完全一样，这里共用同一份类型而不是各自重新声明一遍。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
}
