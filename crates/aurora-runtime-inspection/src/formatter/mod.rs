//! Formatter module boundaries without formatting behavior.

pub mod json;
pub mod text;

pub use json::JsonFormatter;
pub use text::TextFormatter;
