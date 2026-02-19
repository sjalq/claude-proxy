//! Translate Anthropic Messages API requests into `OpenAI` Chat Completions requests.
//!
//! Handles system messages, multi-part content (text, images), tool use, tool results,
//! and tool choice mapping. A single Anthropic message can expand into multiple `OpenAI`
//! messages (e.g. a user message with `tool_result` blocks becomes separate `tool`-role messages).

use std::collections::HashMap;
use std::hash::BuildHasher;

use super::anthropic_types::{
    ContentBlock, Message, MessagesRequest, Role, ToolChoice, ToolChoiceAuto, ToolChoiceSpecific,
};
use super::openai_types::{
    ChatCompletionRequest, ChatContent, ChatFunction, ChatMessage, ChatTool, ChatToolCall,
    ChatToolCallFunction, ChatToolChoice, ChatToolChoiceFunction, ChatToolChoiceSpecific,
    ContentPart, ImageUrlDetail, StreamOptions,
};

/// Translate an Anthropic Messages API request into an `OpenAI` Chat Completions request.
/// Pure function: takes the request + model mapping, returns the translated request.
pub fn anthropic_to_openai<S: BuildHasher>(
    req: &MessagesRequest,
    model_map: &HashMap<String, String, S>,
) -> ChatCompletionRequest {
    let target_model = model_map
        .get(&req.model)
        .cloned()
        .unwrap_or_else(|| req.model.clone());

    let mut messages = Vec::new();

    if let Some(ref system) = req.system {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: Some(ChatContent::Text(system.as_text())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    for msg in &req.messages {
        let mut translated = translate_message(msg);
        messages.append(&mut translated);
    }

    let tools = req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| ChatTool {
                tool_type: "function".to_string(),
                function: ChatFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect()
    });

    let tool_choice = req.tool_choice.as_ref().map(translate_tool_choice);

    let stream_options = req.stream.filter(|s| *s).map(|_| StreamOptions {
        include_usage: true,
    });

    let user = req.metadata.as_ref().and_then(|m| m.user_id.clone());

    ChatCompletionRequest {
        model: target_model,
        messages,
        max_tokens: Some(req.max_tokens),
        temperature: req.temperature,
        top_p: req.top_p,
        stream: req.stream,
        stream_options,
        tools,
        tool_choice,
        stop: req.stop_sequences.clone(),
        user,
    }
}

/// A single Anthropic message can expand to multiple `OpenAI` messages
/// (e.g. a user message with `tool_results` becomes separate tool-role messages).
fn translate_message(msg: &Message) -> Vec<ChatMessage> {
    let blocks = msg.content.blocks();

    match msg.role {
        Role::User => translate_user_message(&blocks),
        Role::Assistant => translate_assistant_message(&blocks),
    }
}

fn translate_user_message(blocks: &[ContentBlock]) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    let mut content_parts: Vec<ContentPart> = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                content_parts.push(ContentPart::Text { text: text.clone() });
            }
            ContentBlock::Image { source } => {
                let data_uri = format!("data:{};base64,{}", source.media_type, source.data);
                content_parts.push(ContentPart::ImageUrl {
                    image_url: ImageUrlDetail {
                        url: data_uri,
                        detail: None,
                    },
                });
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                // Flush any accumulated content parts as a user message first
                if !content_parts.is_empty() {
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: Some(collapse_content_parts(&content_parts)),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                    content_parts.clear();
                }

                let result_text = tool_result_to_string(content.as_ref(), *is_error);

                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(ChatContent::Text(result_text)),
                    tool_calls: None,
                    tool_call_id: Some(tool_use_id.clone()),
                    name: None,
                });
            }
            ContentBlock::Thinking { .. } | ContentBlock::ToolUse { .. } => {}
        }
    }

    if !content_parts.is_empty() {
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: Some(collapse_content_parts(&content_parts)),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    // If nothing was produced (empty message), emit an empty user message
    if messages.is_empty() {
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: Some(ChatContent::Text(String::new())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    messages
}

fn translate_assistant_message(blocks: &[ContentBlock]) -> Vec<ChatMessage> {
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ChatToolCall> = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                text_parts.push(text.clone());
            }
            ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(ChatToolCall {
                    id: id.clone(),
                    call_type: "function".to_string(),
                    function: ChatToolCallFunction {
                        name: name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    },
                });
            }
            ContentBlock::Thinking { .. }
            | ContentBlock::Image { .. }
            | ContentBlock::ToolResult { .. } => {}
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(ChatContent::Text(text_parts.join("")))
    };

    let tool_calls_opt = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    vec![ChatMessage {
        role: "assistant".to_string(),
        content,
        tool_calls: tool_calls_opt,
        tool_call_id: None,
        name: None,
    }]
}

fn collapse_content_parts(parts: &[ContentPart]) -> ChatContent {
    if parts.len() == 1 {
        if let ContentPart::Text { text } = &parts[0] {
            return ChatContent::Text(text.clone());
        }
    }
    ChatContent::Parts(parts.to_vec())
}

fn tool_result_to_string(
    content: Option<&super::anthropic_types::ToolResultContent>,
    is_error: Option<bool>,
) -> String {
    let prefix = if is_error == Some(true) {
        "ERROR: "
    } else {
        ""
    };

    match content {
        Some(super::anthropic_types::ToolResultContent::Text(t)) => {
            format!("{prefix}{t}")
        }
        Some(super::anthropic_types::ToolResultContent::Blocks(blocks)) => {
            let text: String = blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{prefix}{text}")
        }
        None => format!("{prefix}(no content)"),
    }
}

fn translate_tool_choice(tc: &ToolChoice) -> ChatToolChoice {
    match tc {
        ToolChoice::Auto(ToolChoiceAuto { choice_type }) => match choice_type.as_str() {
            "any" => ChatToolChoice::String("required".to_string()),
            "none" => ChatToolChoice::String("none".to_string()),
            _ => ChatToolChoice::String("auto".to_string()),
        },
        ToolChoice::Specific(ToolChoiceSpecific { name, .. }) => {
            ChatToolChoice::Specific(ChatToolChoiceSpecific {
                choice_type: "function".to_string(),
                function: ChatToolChoiceFunction { name: name.clone() },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::anthropic_types::*;

    #[test]
    fn test_simple_text_request() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Hello".to_string()),
            }],
            system: Some(SystemContent::Text("You are helpful".to_string())),
            stream: None,
            temperature: None,
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

        let mut model_map = HashMap::new();
        model_map.insert("claude-sonnet-4-20250514".to_string(), "gpt-4o".to_string());

        let result = anthropic_to_openai(&req, &model_map);

        assert_eq!(result.model, "gpt-4o");
        assert_eq!(result.messages.len(), 2); // system + user
        assert_eq!(result.messages[0].role, "system");
        assert_eq!(result.messages[1].role, "user");
    }

    #[test]
    fn test_tool_result_splits_into_tool_messages() {
        let req = MessagesRequest {
            model: "test".to_string(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "toolu_1".to_string(),
                        content: Some(ToolResultContent::Text("result 1".to_string())),
                        is_error: None,
                    },
                    ContentBlock::Text {
                        text: "Now continue".to_string(),
                    },
                ]),
            }],
            system: None,
            stream: None,
            temperature: None,
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

        let result = anthropic_to_openai(&req, &HashMap::new());

        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].role, "tool");
        assert_eq!(result.messages[0].tool_call_id, Some("toolu_1".to_string()));
        assert_eq!(result.messages[1].role, "user");
    }

    #[test]
    fn test_unmapped_model_passes_through() {
        let req = MessagesRequest {
            model: "some-unknown-model".to_string(),
            max_tokens: 100,
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("hi".to_string()),
            }],
            system: None,
            stream: None,
            temperature: None,
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

        let result = anthropic_to_openai(&req, &HashMap::new());
        assert_eq!(result.model, "some-unknown-model");
    }
}
