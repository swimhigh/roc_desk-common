//! Shared, dependency-light domain primitives for roc_desk tools.
//!
//! This crate deliberately does not depend on the desktop host or Tauri.  Tool
//! crates can therefore depend on it without creating a reverse dependency on
//! the application repository.

pub mod error;
