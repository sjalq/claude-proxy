//! API translation between Anthropic and `OpenAI` formats.
//!
//! The core of the proxy: converts requests, responses, and streaming events
//! between the two API formats. All translation functions are pure (no I/O).

pub mod anthropic_types;
pub mod openai_types;
pub mod request;
pub mod response;
pub mod streaming;
