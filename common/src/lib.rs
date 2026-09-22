//! Optional cross-tool services.
//!
//! Tool-specific implementations are added here only when they are genuinely
//! shared; this crate must never depend on the desktop host or a tool crate.
pub use roc_desk_core as core;
pub mod binary_info;
pub mod encoding;
pub mod fsops;
pub mod jar_info;
pub mod office_convert;
