use super::anthropic_types::{
    ErrorResponse, MessagesResponse, ResponseContentBlock, Usage,
};
use super::openai_types::{ChatCompletionResponse, ChatErrorResponse};

/// Translate an OpenAI Chat Completion response into an Anthropic Messages response.
/// Pure function: original_model is what Claude Code originally requested.
pub fn openai_to_anthropic(
    resp: &ChatCompletionResponse,
    original_model: &str,
) -> MessagesResponse {
    let choice = resp.choices.first();

    let mut content: Vec<ResponseContentBlock> = Vec::new();

    if let Some(c) = choice {
        if let Some(ref text) = c.message.content {
            if !text.is_empty() {
                content.push(ResponseContentBlock::Text { text: text.clone() });
            }
        }

        if let Some(ref tool_calls) = c.message.tool_calls {
            for tc in tool_calls {
                let input: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);

                content.push(ResponseContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input,
                });
            }
        }
    }

    // Ensure at least one content block (Claude Code expects non-empty content)
    if content.is_empty() {
        content.push(ResponseContentBlock::Text {
            text: String::new(),
        });
    }

    let stop_reason = choice
        .and_then(|c| c.finish_reason.as_deref())
        .map(map_finish_reason)
        .unwrap_or_else(|| "end_turn".to_string());

    let usage = resp.usage.as_ref().map_or_else(Usage::default, |u| Usage {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    });

    // Use the OpenAI response ID, prefixed to look like an Anthropic ID
    let id = format!("msg_{}", resp.id.trim_start_matches("chatcmpl-"));

    MessagesResponse {
        id,
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: original_model.to_string(),
        stop_reason: Some(stop_reason),
        stop_sequence: None,
        usage,
    }
}

/// Map OpenAI finish_reason to Anthropic stop_reason
pub fn map_finish_reason(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_string(),
        "length" => "max_tokens".to_string(),
        "tool_calls" | "function_call" => "tool_use".to_string(),
        "content_filter" => "end_turn".to_string(),
        other => other.to_string(),
    }
}

/// Translate an OpenAI error into an Anthropic error response
pub fn openai_error_to_anthropic(err: &ChatErrorResponse) -> ErrorResponse {
    let error_type = match err.error.error_type.as_str() {
        "invalid_request_error" => "invalid_request_error",
        "rate_limit_error" | "rate_limit_exceeded" => "rate_limit_error",
        "server_error" | "internal_error" => "api_error",
        _ => "api_error",
    };

    ErrorResponse::new(error_type, &err.error.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::openai_types::*;

    fn make_response(content: Option<String>, finish_reason: Option<String>) -> ChatCompletionResponse {
        ChatCompletionResponse {
            id: "chatcmpl-abc123".to_string(),
            object: "chat.completion".to_string(),
            created: 0,
            model: "gpt-4o".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".to_string(),
                    content,
                    tool_calls: None,
                },
                finish_reason,
            }],
            usage: Some(ChatUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            }),
        }
    }

    #[test]
    fn test_simple_text_response() {
        let resp = make_response(Some("Hello!".to_string()), Some("stop".to_string()));
        let result = openai_to_anthropic(&resp, "claude-sonnet-4-20250514");

        assert_eq!(result.role, "assistant");
        assert_eq!(result.model, "claude-sonnet-4-20250514");
        assert_eq!(result.stop_reason, Some("end_turn".to_string()));
        assert_eq!(result.content.len(), 1);

        if let ResponseContentBlock::Text { text } = &result.content[0] {
            assert_eq!(text, "Hello!");
        } else {
            panic!("Expected text content block");
        }

        assert_eq!(result.usage.input_tokens, 10);
        assert_eq!(result.usage.output_tokens, 20);
    }

    #[test]
    fn test_tool_call_response() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-xyz".to_string(),
            object: "chat.completion".to_string(),
            created: 0,
            model: "gpt-4o".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".to_string(),
                    content: Some("Let me check.".to_string()),
                    tool_calls: Some(vec![ChatToolCall {
                        id: "call_abc".to_string(),
                        call_type: "function".to_string(),
                        function: ChatToolCallFunction {
                            name: "get_weather".to_string(),
                            arguments: "{\"city\":\"London\"}".to_string(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: None,
        };

        let result = openai_to_anthropic(&resp, "test-model");

        assert_eq!(result.content.len(), 2);
        assert_eq!(result.stop_reason, Some("tool_use".to_string()));

        if let ResponseContentBlock::ToolUse { id, name, input } = &result.content[1] {
            assert_eq!(id, "call_abc");
            assert_eq!(name, "get_weather");
            assert_eq!(input["city"], "London");
        } else {
            panic!("Expected tool_use content block");
        }
    }

    #[test]
    fn test_finish_reason_mapping() {
        assert_eq!(map_finish_reason("stop"), "end_turn");
        assert_eq!(map_finish_reason("length"), "max_tokens");
        assert_eq!(map_finish_reason("tool_calls"), "tool_use");
        assert_eq!(map_finish_reason("unknown"), "unknown");
    }
}
