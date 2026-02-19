//! Demonstrate using the translation layer without a server.
//!
//! Usage:
//!   `cargo run --example translate_only`

use claude_proxy::translate::anthropic_types::{
    Message, MessageContent, MessagesRequest, Role, SystemContent,
};
use claude_proxy::translate::openai_types::{
    ChatCompletionChunk, ChatCompletionResponse, ChatUsage, Choice, ChoiceMessage, ChunkChoice,
    ChunkDelta,
};
use claude_proxy::translate::request::anthropic_to_openai;
use claude_proxy::translate::response::openai_to_anthropic;
use claude_proxy::translate::streaming::StreamTranslator;
use std::collections::HashMap;

fn main() {
    // Build an Anthropic Messages API request (what Claude Code sends)
    let anthropic_req = MessagesRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        max_tokens: 1024,
        messages: vec![
            Message {
                role: Role::User,
                content: MessageContent::Text("What is the capital of France?".to_string()),
            },
            Message {
                role: Role::Assistant,
                content: MessageContent::Text("The capital of France is Paris.".to_string()),
            },
            Message {
                role: Role::User,
                content: MessageContent::Text("And Germany?".to_string()),
            },
        ],
        system: Some(SystemContent::Text(
            "You are a geography expert. Be concise.".to_string(),
        )),
        stream: Some(true),
        temperature: Some(0.7),
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
    };

    // Translate to OpenAI format
    let model_map = HashMap::from([("claude-sonnet-4-20250514".to_string(), "gpt-4o".to_string())]);

    let openai_req = anthropic_to_openai(&anthropic_req, &model_map);

    println!("=== Translated Request (OpenAI format) ===");
    println!("{}", serde_json::to_string_pretty(&openai_req).unwrap());

    // Simulate an OpenAI response and translate back
    let openai_resp = ChatCompletionResponse {
        id: "chatcmpl-demo".to_string(),
        object: "chat.completion".to_string(),
        created: 0,
        model: "gpt-4o".to_string(),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                role: "assistant".to_string(),
                content: Some("The capital of Germany is Berlin.".to_string()),
                reasoning_content: None,
                tool_calls: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(ChatUsage {
            prompt_tokens: 42,
            completion_tokens: 8,
            total_tokens: 50,
        }),
    };

    let anthropic_resp = openai_to_anthropic(&openai_resp, "claude-sonnet-4-20250514");

    println!();
    println!("=== Translated Response (Anthropic format) ===");
    println!("{}", serde_json::to_string_pretty(&anthropic_resp).unwrap());

    // Demonstrate the streaming translator
    println!();
    println!("=== Streaming Translation Demo ===");

    let mut translator = StreamTranslator::new("claude-sonnet-4-20250514");

    let chunks = vec![
        ChunkDelta {
            role: Some("assistant".to_string()),
            content: Some("The".to_string()),
            reasoning_content: None,
            tool_calls: None,
        },
        ChunkDelta {
            role: None,
            content: Some(" capital".to_string()),
            reasoning_content: None,
            tool_calls: None,
        },
        ChunkDelta {
            role: None,
            content: Some(" is Berlin.".to_string()),
            reasoning_content: None,
            tool_calls: None,
        },
    ];

    for (i, delta) in chunks.into_iter().enumerate() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-demo".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "gpt-4o".to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta,
                finish_reason: None,
            }],
            usage: None,
        };

        let events = translator.process_chunk(&chunk);
        for event in &events {
            println!("  chunk {} -> event: {}", i, event.event_name());
        }
    }

    let finish_chunk = ChatCompletionChunk {
        id: "chatcmpl-demo".to_string(),
        object: "chat.completion.chunk".to_string(),
        created: 0,
        model: "gpt-4o".to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta::default(),
            finish_reason: Some("stop".to_string()),
        }],
        usage: None,
    };

    let events = translator.process_chunk(&finish_chunk);
    for event in &events {
        println!("  finish -> event: {}", event.event_name());
    }

    println!();
    println!("Done! The translation layer works without any network calls.");
}
