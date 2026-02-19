//! Core proxy logic: forward requests to the configured provider, translating
//! between Anthropic and OpenAI formats as needed.
//!
//! Supports non-streaming, streaming (SSE), and direct passthrough modes.
//! Includes automatic retry with exponential backoff for transient errors.

use crate::config::ProxyConfig;
use crate::error::{ProxyError, Result};
use crate::logging::SharedLogger;
use crate::translate::anthropic_types::{ErrorResponse, MessagesRequest, MessagesResponse};
use crate::translate::openai_types::{
    ChatCompletionChunk, ChatCompletionResponse, ChatErrorResponse,
};
use crate::translate::request::anthropic_to_openai;
use crate::translate::response::{openai_error_to_anthropic, openai_to_anthropic};
use crate::translate::streaming::StreamTranslator;

use bytes::Bytes;
use futures::stream::{self, Stream};
#[allow(unused_imports)]
use futures::StreamExt;
use std::pin::Pin;

const MAX_RETRIES: u32 = 2;
const RETRYABLE_STATUSES: &[u16] = &[429, 500, 502, 503, 504];

/// Outcome of proxying a non-streaming request.
pub enum ProxyResult {
    /// Successful response, translated to Anthropic format.
    Success(MessagesResponse),
    /// Provider returned an error, translated to Anthropic error format.
    Error(ErrorResponse, u16),
}

/// A single SSE event ready for emission.
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

/// Stream of SSE events for a streaming response.
pub type SseStream =
    Pin<Box<dyn Stream<Item = std::result::Result<SseEvent, std::io::Error>> + Send>>;

/// Forward a non-streaming Anthropic request through the configured provider.
///
/// Translates the request to OpenAI format, sends it, translates the response
/// back to Anthropic format. Retries on transient errors (429, 5xx).
pub async fn proxy_non_streaming(
    req: &MessagesRequest,
    config: &ProxyConfig,
    client: &reqwest::Client,
    logger: &SharedLogger,
) -> Result<ProxyResult> {
    let api_key = config.resolve_api_key()?;
    let base_url = config.effective_base_url()?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let openai_req = anthropic_to_openai(req, &config.models);

    logger.info("proxy", format!("POST {} model={}", url, openai_req.model));

    let body = serde_json::to_vec(&openai_req)
        .map_err(|e| ProxyError::translation(format!("Failed to serialize request: {}", e)))?;

    let response = send_with_retry(
        client,
        &url,
        &api_key,
        &body,
        logger,
    )
    .await?;

    let status = response.status().as_u16();
    let resp_body = response.text().await.map_err(|e| {
        ProxyError::provider(format!("Failed to read response body: {}", e))
    })?;

    logger.debug(
        "proxy",
        format!("Response status={} body_len={}", status, resp_body.len()),
    );

    if status >= 400 {
        if let Ok(err) = serde_json::from_str::<ChatErrorResponse>(&resp_body) {
            let anthropic_err = openai_error_to_anthropic(&err);
            logger.warn("proxy", format!("Provider error: {}", err.error.message));
            return Ok(ProxyResult::Error(anthropic_err, status));
        }

        let anthropic_err = ErrorResponse::api_error(format!(
            "Provider returned status {}: {}",
            status,
            truncate(&resp_body, 500)
        ));
        return Ok(ProxyResult::Error(anthropic_err, status));
    }

    let openai_resp: ChatCompletionResponse =
        serde_json::from_str(&resp_body).map_err(|e| {
            ProxyError::translation(format!(
                "Failed to parse provider response: {}. Body: {}",
                e,
                truncate(&resp_body, 300)
            ))
        })?;

    let anthropic_resp = openai_to_anthropic(&openai_resp, &req.model);

    logger.info(
        "proxy",
        format!(
            "Completed: in={} out={} tokens",
            anthropic_resp.usage.input_tokens, anthropic_resp.usage.output_tokens
        ),
    );

    Ok(ProxyResult::Success(anthropic_resp))
}

/// Forward a streaming Anthropic request, returning a stream of Anthropic SSE events.
///
/// The provider's OpenAI-format SSE chunks are translated into Anthropic-format
/// events on the fly via [`StreamTranslator`].
pub async fn proxy_streaming(
    req: &MessagesRequest,
    config: &ProxyConfig,
    client: &reqwest::Client,
    logger: &SharedLogger,
) -> Result<SseStream> {
    let api_key = config.resolve_api_key()?;
    let base_url = config.effective_base_url()?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let openai_req = anthropic_to_openai(req, &config.models);

    logger.info(
        "proxy",
        format!("POST {} model={} (streaming)", url, openai_req.model),
    );

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&openai_req)
        .send()
        .await
        .map_err(|e| ProxyError::provider(format!("Streaming request failed: {}", e)))?;

    let status = response.status().as_u16();

    if status >= 400 {
        let body = response.text().await.unwrap_or_default();
        logger.warn(
            "proxy",
            format!("Streaming error status={}: {}", status, truncate(&body, 300)),
        );

        let error_event = if let Ok(err) = serde_json::from_str::<ChatErrorResponse>(&body) {
            openai_error_to_anthropic(&err)
        } else {
            ErrorResponse::api_error(format!("Provider returned status {}", status))
        };

        let error_json = serde_json::to_string(&error_event).unwrap_or_default();
        let event = SseEvent {
            event: "error".to_string(),
            data: error_json,
        };

        return Ok(Box::pin(stream::once(async move { Ok(event) })));
    }

    let original_model = req.model.clone();
    let logger_clone = logger.clone();
    let byte_stream = response.bytes_stream();

    let event_stream = sse_translate_stream(byte_stream, original_model, logger_clone);

    Ok(Box::pin(event_stream))
}

/// Parse an OpenAI SSE byte stream and translate chunks into Anthropic SSE events.
fn sse_translate_stream(
    byte_stream: impl Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send + 'static,
    model: String,
    logger: SharedLogger,
) -> impl Stream<Item = std::result::Result<SseEvent, std::io::Error>> + Send + 'static {
    async_stream::stream! {
        let mut translator = StreamTranslator::new(&model);
        let mut buffer = String::new();

        tokio::pin!(byte_stream);

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    logger.error("stream", format!("Byte stream error: {}", e));
                    break;
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                let data = if let Some(stripped) = line.strip_prefix("data: ") {
                    stripped.trim()
                } else if let Some(stripped) = line.strip_prefix("data:") {
                    stripped.trim()
                } else {
                    // Skip non-data SSE lines (event:, id:, retry:, comments)
                    continue;
                };

                if data == "[DONE]" {
                    let events = translator.finish();
                    for event in events {
                        if let Ok(json) = serde_json::to_string(&event) {
                            yield Ok(SseEvent {
                                event: event.event_name().to_string(),
                                data: json,
                            });
                        }
                    }
                    break;
                }

                let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(e) => {
                        logger.debug("stream", format!("Skipping unparseable chunk: {}", e));
                        continue;
                    }
                };

                let events = translator.process_chunk(&chunk);
                for event in events {
                    if let Ok(json) = serde_json::to_string(&event) {
                        yield Ok(SseEvent {
                            event: event.event_name().to_string(),
                            data: json,
                        });
                    }
                }
            }
        }

        // Ensure stream is closed even if [DONE] was missing
        let final_events = translator.finish();
        for event in final_events {
            if let Ok(json) = serde_json::to_string(&event) {
                yield Ok(SseEvent {
                    event: event.event_name().to_string(),
                    data: json,
                });
            }
        }

        logger.info("stream", "Stream completed");
    }
}

/// Forward an Anthropic-format request directly (passthrough mode for Anthropic provider).
pub async fn proxy_passthrough(
    body: Bytes,
    headers: &reqwest::header::HeaderMap,
    config: &ProxyConfig,
    client: &reqwest::Client,
    logger: &SharedLogger,
) -> Result<(u16, reqwest::header::HeaderMap, Bytes)> {
    let api_key = config.resolve_api_key()?;
    let base_url = config.effective_base_url()?;
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

    logger.info("proxy", format!("Passthrough POST {}", url));

    let mut req_builder = client
        .post(&url)
        .header("x-api-key", &api_key)
        .header("Content-Type", "application/json");

    if let Some(version) = headers.get("anthropic-version") {
        req_builder = req_builder.header("anthropic-version", version);
    }

    let response = req_builder
        .body(body)
        .send()
        .await
        .map_err(|e| ProxyError::provider(format!("Passthrough request failed: {}", e)))?;

    let status = response.status().as_u16();
    let resp_headers = response.headers().clone();
    let resp_body = response
        .bytes()
        .await
        .map_err(|e| ProxyError::provider(format!("Failed to read passthrough response: {}", e)))?;

    logger.info(
        "proxy",
        format!(
            "Passthrough response: status={} len={}",
            status,
            resp_body.len()
        ),
    );

    Ok((status, resp_headers, resp_body))
}

/// Send a POST request with automatic retry on transient failures.
///
/// Retries up to [`MAX_RETRIES`] times on status codes in [`RETRYABLE_STATUSES`],
/// using exponential backoff starting at 500ms.
async fn send_with_retry(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &[u8],
    logger: &SharedLogger,
) -> Result<reqwest::Response> {
    let mut delay = std::time::Duration::from_millis(500);

    for attempt in 0..=MAX_RETRIES {
        let resp = client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|e| ProxyError::provider(format!("Request failed: {}", e)))?;

        let status = resp.status().as_u16();

        if attempt < MAX_RETRIES && RETRYABLE_STATUSES.contains(&status) {
            logger.warn(
                "retry",
                format!(
                    "Attempt {}/{}: status {}, retrying in {:?}",
                    attempt + 1,
                    MAX_RETRIES + 1,
                    status,
                    delay
                ),
            );
            // Consume the body so the connection can be reused
            let _ = resp.bytes().await;
            tokio::time::sleep(delay).await;
            delay *= 2;
            continue;
        }

        return Ok(resp);
    }

    unreachable!()
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
