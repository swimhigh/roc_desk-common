//! Optional cross-tool services.
//!
//! Tool-specific implementations are added here only when they are genuinely
//! shared; this crate must never depend on the desktop host or a tool crate.
pub use roc_desk_core as core;
pub mod agent_confirm;
pub mod agent_llm;
pub mod agent_todo;
pub mod ai;
pub mod binary_info;
pub mod encoding;
pub mod fsops;
pub mod jar_info;
pub mod office_convert;
pub mod symbols;
