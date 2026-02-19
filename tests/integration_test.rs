use claude_proxy::config::{ParamsConfig, ProviderConfig, ProxyConfig};
use claude_proxy::logging::SharedLogger;
use claude_proxy::proxy;
use claude_proxy::translate::anthropic_types::*;
use futures::StreamExt;
use std::collections::HashMap;

fn fireworks_config() -> ProxyConfig {
    let mut models = HashMap::new();
    models.insert(
        "claude-sonnet-4-20250514".to_string(),
        "accounts/fireworks/models/kimi-k2p5".to_string(),
    );
    models.insert(
        "test-model".to_string(),
        "accounts/fireworks/models/kimi-k2p5".to_string(),
    );

    ProxyConfig {
        port: 0,
        provider: ProviderConfig {
            name: "fireworks".to_string(),
            base_url: Some("https://api.fireworks.ai/inference/v1".to_string()),
            api_key_env: "FIREWORKS_API_KEY".to_string(),
            format: Some("openai".to_string()),
        },
        models,
        params: ParamsConfig {
            drop: vec!["betas".to_string(), "context_management".to_string()],
        },
    }
}

fn simple_request(model: &str, prompt: &str) -> MessagesRequest {
    MessagesRequest {
        model: model.to_string(),
        max_tokens: 50,
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Text(prompt.to_string()),
        }],
        system: Some(SystemContent::Text(
            "You are a helpful assistant. Respond very briefly.".to_string(),
        )),
        stream: None,
        temperature: Some(0.0),
        top_p: None,
        top_k: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        stop_sequences: None,
        thinking: None,
        betas: None,
        context_management: None,
        reasoning_effort: None,
        extra: HashMap::default(),
    }
}

fn streaming_request(model: &str, prompt: &str) -> MessagesRequest {
    let mut req = simple_request(model, prompt);
    req.stream = Some(true);
    req
}

fn tool_request() -> MessagesRequest {
    MessagesRequest {
        model: "test-model".to_string(),
        max_tokens: 200,
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Text(
                "What's the weather in London? Use the get_weather tool.".to_string(),
            ),
        }],
        system: None,
        stream: None,
        temperature: Some(0.0),
        top_p: None,
        top_k: None,
        tools: Some(vec![Tool {
            name: "get_weather".to_string(),
            description: Some("Get current weather for a city".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": {
                        "type": "string",
                        "description": "City name"
                    }
                },
                "required": ["city"]
            }),
        }]),
        tool_choice: Some(ToolChoice::Auto(ToolChoiceAuto {
            choice_type: "auto".to_string(),
        })),
        metadata: None,
        stop_sequences: None,
        thinking: None,
        betas: None,
        context_management: None,
        reasoning_effort: None,
        extra: HashMap::default(),
    }
}

// ────────────────────────────────────────────────────────────────
// Unit tests (no API key needed)
// ────────────────────────────────────────────────────────────────

#[test]
fn test_request_translation_roundtrip() {
    let req = simple_request("claude-sonnet-4-20250514", "Hello");
    let mut model_map = HashMap::new();
    model_map.insert("claude-sonnet-4-20250514".to_string(), "gpt-4o".to_string());

    let openai_req = claude_proxy::translate::request::anthropic_to_openai(&req, &model_map);

    assert_eq!(openai_req.model, "gpt-4o");
    assert_eq!(openai_req.messages.len(), 2);
    assert_eq!(openai_req.messages[0].role, "system");
    assert_eq!(openai_req.messages[1].role, "user");
    assert_eq!(openai_req.max_tokens, Some(50));
}

#[test]
fn test_response_translation() {
    use claude_proxy::translate::openai_types::*;
    use claude_proxy::translate::response::openai_to_anthropic;

    let openai_resp = ChatCompletionResponse {
        id: "chatcmpl-test".to_string(),
        object: "chat.completion".to_string(),
        created: 12345,
        model: "gpt-4o".to_string(),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                role: "assistant".to_string(),
                content: Some("Hello there!".to_string()),
                reasoning_content: None,
                tool_calls: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(ChatUsage {
            prompt_tokens: 5,
            completion_tokens: 3,
            total_tokens: 8,
        }),
    };

    let result = openai_to_anthropic(&openai_resp, "claude-sonnet-4-20250514");

    assert_eq!(result.response_type, "message");
    assert_eq!(result.role, "assistant");
    assert_eq!(result.model, "claude-sonnet-4-20250514");
    assert_eq!(result.stop_reason, Some("end_turn".to_string()));
    assert_eq!(result.usage.input_tokens, 5);
    assert_eq!(result.usage.output_tokens, 3);
}

#[test]
fn test_stream_translator_basic() {
    use claude_proxy::translate::openai_types::*;
    use claude_proxy::translate::streaming::StreamTranslator;

    let mut translator = StreamTranslator::new("test-model");

    let chunk = ChatCompletionChunk {
        id: "c1".to_string(),
        object: "chat.completion.chunk".to_string(),
        created: 0,
        model: "test".to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: Some("assistant".to_string()),
                content: Some("Hi".to_string()),
                reasoning_content: None,
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };

    let events = translator.process_chunk(&chunk);
    assert!(!events.is_empty());

    let has_message_start = events.iter().any(|e| e.event_name() == "message_start");
    let has_text_delta = events
        .iter()
        .any(|e| e.event_name() == "content_block_delta");
    assert!(has_message_start);
    assert!(has_text_delta);

    let final_events = translator.finish();
    let has_stop = final_events
        .iter()
        .any(|e| e.event_name() == "message_stop");
    assert!(has_stop);
}

// ────────────────────────────────────────────────────────────────
// Integration tests (need FIREWORKS_API_KEY)
// ────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires FIREWORKS_API_KEY"]
async fn test_non_streaming_fireworks() {
    let config = fireworks_config();
    let client = reqwest::Client::new();
    let logger = SharedLogger::new("/tmp/claude-proxy-test.log").unwrap();
    let req = simple_request("test-model", "Say 'hello' and nothing else.");

    let result = proxy::proxy_non_streaming(&req, &config, &client, &logger).await;

    match result {
        Ok(proxy::ProxyResult::Success(resp)) => {
            assert_eq!(resp.response_type, "message");
            assert_eq!(resp.role, "assistant");
            assert!(!resp.content.is_empty());
            println!("Response: {:?}", resp.content);
            println!(
                "Usage: in={} out={}",
                resp.usage.input_tokens, resp.usage.output_tokens
            );
        }
        Ok(proxy::ProxyResult::Error(err, status)) => {
            panic!("Provider error ({status}): {err:?}");
        }
        Err(e) => {
            panic!("Proxy error: {e}");
        }
    }
}

#[tokio::test]
#[ignore = "requires FIREWORKS_API_KEY"]
async fn test_streaming_fireworks() {
    let config = fireworks_config();
    let client = reqwest::Client::new();
    let logger = SharedLogger::new("/tmp/claude-proxy-test-stream.log").unwrap();
    let req = streaming_request("test-model", "Count from 1 to 5.");

    let stream = proxy::proxy_streaming(&req, &config, &client, &logger)
        .await
        .expect("Failed to start stream");

    let events: Vec<_> = stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect();

    assert!(!events.is_empty(), "Stream produced no events");

    let event_names: Vec<&str> = events.iter().map(|e| e.event.as_str()).collect();
    println!("Stream events: {event_names:?}");

    assert!(
        event_names.contains(&"message_start"),
        "Missing message_start"
    );
    assert!(
        event_names.contains(&"message_stop"),
        "Missing message_stop"
    );
    assert!(
        event_names.contains(&"content_block_delta"),
        "Missing content deltas"
    );
}

#[tokio::test]
#[ignore = "requires FIREWORKS_API_KEY"]
async fn test_tool_use_fireworks() {
    let config = fireworks_config();
    let client = reqwest::Client::new();
    let logger = SharedLogger::new("/tmp/claude-proxy-test-tools.log").unwrap();
    let req = tool_request();

    let result = proxy::proxy_non_streaming(&req, &config, &client, &logger).await;

    match result {
        Ok(proxy::ProxyResult::Success(resp)) => {
            println!("Tool response: {:?}", resp.content);

            let has_tool_use = resp
                .content
                .iter()
                .any(|b| matches!(b, ResponseContentBlock::ToolUse { .. }));

            // The model may or may not call the tool - just verify we got a valid response
            assert_eq!(resp.response_type, "message");
            if has_tool_use {
                println!("Model correctly used the tool");
                assert_eq!(resp.stop_reason, Some("tool_use".to_string()));
            } else {
                println!("Model responded with text (didn't use tool)");
            }
        }
        Ok(proxy::ProxyResult::Error(err, status)) => {
            panic!("Provider error ({status}): {err:?}");
        }
        Err(e) => {
            panic!("Proxy error: {e}");
        }
    }
}

#[tokio::test]
#[ignore = "requires FIREWORKS_API_KEY"]
async fn test_full_server_roundtrip() {
    let config = fireworks_config();
    let logger = SharedLogger::new("/tmp/claude-proxy-test-server.log").unwrap();
    let client = reqwest::Client::new();

    let state = std::sync::Arc::new(claude_proxy::AppState {
        config: ProxyConfig { port: 0, ..config },
        client: client.clone(),
        logger,
    });

    let app = claude_proxy::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Test health endpoint
    let health_resp = client
        .get(format!("http://{addr}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(health_resp.status(), 200);

    // Test non-streaming message
    let req_body = serde_json::json!({
        "model": "test-model",
        "max_tokens": 30,
        "messages": [{"role": "user", "content": "Say 'pong'"}],
    });

    let msg_resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("Content-Type", "application/json")
        .json(&req_body)
        .send()
        .await
        .unwrap();

    assert_eq!(msg_resp.status(), 200);

    let body: serde_json::Value = msg_resp.json().await.unwrap();
    assert_eq!(body["type"], "message");
    assert_eq!(body["role"], "assistant");
    println!("Server roundtrip response: {body}");
}
