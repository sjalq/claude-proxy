//! State machine for translating OpenAI streaming chunks into Anthropic SSE events.
//!
//! The [`StreamTranslator`] processes OpenAI `ChatCompletionChunk`s one at a time,
//! maintaining state about which content blocks are open, and emitting the
//! corresponding Anthropic stream events (`message_start`, `content_block_delta`, etc.).

use super::anthropic_types::{
    Delta, DeltaUsage, MessageDeltaBody, MessagesResponse, ResponseContentBlock, StreamEvent, Usage,
};
use super::openai_types::ChatCompletionChunk;
use super::response::map_finish_reason;

/// Tracks state of an in-progress tool call being streamed
#[derive(Debug, Clone)]
struct ActiveToolCall {
    anthropic_block_index: usize,
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
    emitted_start: bool,
}

/// State machine that translates OpenAI streaming chunks into Anthropic SSE events.
///
/// Usage:
///   let mut translator = StreamTranslator::new("claude-sonnet-4-20250514");
///   for chunk in openai_chunks {
///       let events = translator.process_chunk(&chunk);
///       // send each event as SSE
///   }
///   let final_events = translator.finish();
#[derive(Debug)]
pub struct StreamTranslator {
    model: String,
    msg_id: String,
    started: bool,
    finished: bool,
    content_block_index: usize,
    in_text_block: bool,
    active_tool_calls: Vec<ActiveToolCall>,
    input_tokens: u64,
    output_tokens: u64,
}

impl StreamTranslator {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            msg_id: format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
            started: false,
            finished: false,
            content_block_index: 0,
            in_text_block: false,
            active_tool_calls: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    /// Process a single OpenAI streaming chunk, returning zero or more Anthropic SSE events.
    pub fn process_chunk(&mut self, chunk: &ChatCompletionChunk) -> Vec<StreamEvent> {
        if self.finished {
            return Vec::new();
        }

        let mut events = Vec::new();

        // Capture usage if provided
        if let Some(ref usage) = chunk.usage {
            self.input_tokens = usage.prompt_tokens;
            self.output_tokens = usage.completion_tokens;
        }

        // Emit message_start on first chunk
        if !self.started {
            events.push(self.make_message_start());
            events.push(StreamEvent::Ping);
            self.started = true;
        }

        let choice = match chunk.choices.first() {
            Some(c) => c,
            None => return events,
        };

        // Handle text content deltas.
        // Some reasoning models (Kimi K2.5, DeepSeek R1) stream chain-of-thought
        // in `reasoning_content` and the final answer in `content`. We emit both
        // as text deltas so Claude Code sees the full response.
        let effective_content = choice
            .delta
            .content
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                choice
                    .delta
                    .reasoning_content
                    .as_deref()
                    .filter(|s| !s.is_empty())
            });

        if let Some(content) = effective_content {
            if !self.in_text_block {
                events.push(StreamEvent::ContentBlockStart {
                    index: self.content_block_index,
                    content_block: ResponseContentBlock::Text {
                        text: String::new(),
                    },
                });
                self.in_text_block = true;
            }

            events.push(StreamEvent::ContentBlockDelta {
                index: self.content_block_index,
                delta: Delta::TextDelta {
                    text: content.to_string(),
                },
            });
        }

        // Handle tool call deltas
        if let Some(ref tool_calls) = choice.delta.tool_calls {
            for tc in tool_calls {
                let tc_index = tc.index as usize;

                // Check if this is a new tool call (has an id)
                if tc.id.is_some() {
                    // Close text block if open
                    if self.in_text_block {
                        events.push(StreamEvent::ContentBlockStop {
                            index: self.content_block_index,
                        });
                        self.content_block_index += 1;
                        self.in_text_block = false;
                    }

                    let tool_id = tc.id.clone().unwrap_or_default();
                    let tool_name = tc
                        .function
                        .as_ref()
                        .and_then(|f| f.name.clone())
                        .unwrap_or_default();

                    // Emit content_block_start for this tool_use
                    events.push(StreamEvent::ContentBlockStart {
                        index: self.content_block_index,
                        content_block: ResponseContentBlock::ToolUse {
                            id: tool_id.clone(),
                            name: tool_name.clone(),
                            input: serde_json::Value::Object(serde_json::Map::new()),
                        },
                    });

                    // Ensure our active_tool_calls vec is big enough
                    while self.active_tool_calls.len() <= tc_index {
                        self.active_tool_calls.push(ActiveToolCall {
                            anthropic_block_index: 0,
                            id: String::new(),
                            name: String::new(),
                            emitted_start: false,
                        });
                    }

                    self.active_tool_calls[tc_index] = ActiveToolCall {
                        anthropic_block_index: self.content_block_index,
                        id: tool_id,
                        name: tool_name,
                        emitted_start: true,
                    };
                }

                // Emit argument deltas
                if let Some(ref func) = tc.function {
                    if let Some(ref args) = func.arguments {
                        if !args.is_empty() {
                            let block_idx = if tc_index < self.active_tool_calls.len() {
                                self.active_tool_calls[tc_index].anthropic_block_index
                            } else {
                                self.content_block_index
                            };

                            events.push(StreamEvent::ContentBlockDelta {
                                index: block_idx,
                                delta: Delta::InputJsonDelta {
                                    partial_json: args.clone(),
                                },
                            });
                        }
                    }
                }
            }
        }

        // Handle finish
        if let Some(ref reason) = choice.finish_reason {
            events.append(&mut self.make_finish_events(reason));
        }

        events
    }

    /// Call when the stream ends (on `[DONE]`) to flush any remaining events.
    pub fn finish(&mut self) -> Vec<StreamEvent> {
        if self.finished {
            return Vec::new();
        }

        if !self.started {
            let mut events = vec![self.make_message_start()];
            events.append(&mut self.make_finish_events("stop"));
            return events;
        }

        // If we haven't finished normally (no finish_reason seen), close now
        self.make_finish_events("stop")
    }

    fn make_message_start(&self) -> StreamEvent {
        StreamEvent::MessageStart {
            message: MessagesResponse {
                id: self.msg_id.clone(),
                response_type: "message".to_string(),
                role: "assistant".to_string(),
                content: Vec::new(),
                model: self.model.clone(),
                stop_reason: None,
                stop_sequence: None,
                usage: Usage {
                    input_tokens: self.input_tokens,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
        }
    }

    fn make_finish_events(&mut self, reason: &str) -> Vec<StreamEvent> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;

        let mut events = Vec::new();

        // Close text block if open
        if self.in_text_block {
            events.push(StreamEvent::ContentBlockStop {
                index: self.content_block_index,
            });
            self.in_text_block = false;
        }

        // Close any open tool blocks
        for tc in &self.active_tool_calls {
            if tc.emitted_start {
                events.push(StreamEvent::ContentBlockStop {
                    index: tc.anthropic_block_index,
                });
            }
        }
        self.active_tool_calls.clear();

        events.push(StreamEvent::MessageDelta {
            delta: MessageDeltaBody {
                stop_reason: Some(map_finish_reason(reason)),
                stop_sequence: None,
            },
            usage: DeltaUsage {
                output_tokens: self.output_tokens,
            },
        });

        events.push(StreamEvent::MessageStop);

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::openai_types::*;

    fn text_chunk(id: &str, content: &str, finish: Option<&str>) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: id.to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "test".to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: Some(content.to_string()),
                    reasoning_content: None,
                    tool_calls: None,
                },
                finish_reason: finish.map(String::from),
            }],
            usage: None,
        }
    }

    #[test]
    fn test_simple_text_stream() {
        let mut translator = StreamTranslator::new("test-model");

        // First chunk
        let events = translator.process_chunk(&text_chunk("c1", "Hello", None));
        assert!(events.len() >= 3); // message_start, ping, block_start, delta

        let event_names: Vec<&str> = events.iter().map(|e| e.event_name()).collect();
        assert!(event_names.contains(&"message_start"));
        assert!(event_names.contains(&"content_block_start"));
        assert!(event_names.contains(&"content_block_delta"));

        // Second chunk
        let events = translator.process_chunk(&text_chunk("c1", " world", None));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_name(), "content_block_delta");

        // Finish
        let events = translator.process_chunk(&text_chunk("c1", "", Some("stop")));
        let event_names: Vec<&str> = events.iter().map(|e| e.event_name()).collect();
        assert!(event_names.contains(&"content_block_stop"));
        assert!(event_names.contains(&"message_delta"));
        assert!(event_names.contains(&"message_stop"));
    }

    #[test]
    fn test_tool_call_stream() {
        let mut translator = StreamTranslator::new("test-model");

        // First chunk: text
        let _ = translator.process_chunk(&text_chunk("c1", "Checking...", None));

        // Tool call start
        let tool_chunk = ChatCompletionChunk {
            id: "c1".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "test".to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: None,
                    reasoning_content: None,
                    tool_calls: Some(vec![ChunkToolCall {
                        index: 0,
                        id: Some("call_abc".to_string()),
                        call_type: Some("function".to_string()),
                        function: Some(ChunkToolCallFunction {
                            name: Some("search".to_string()),
                            arguments: Some("{\"q\"".to_string()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
            usage: None,
        };

        let events = translator.process_chunk(&tool_chunk);
        let event_names: Vec<&str> = events.iter().map(|e| e.event_name()).collect();
        assert!(event_names.contains(&"content_block_stop")); // closes text block
        assert!(event_names.contains(&"content_block_start")); // opens tool block
        assert!(event_names.contains(&"content_block_delta")); // argument delta
    }

    #[test]
    fn test_finish_without_chunks() {
        let mut translator = StreamTranslator::new("test-model");
        let events = translator.finish();

        let event_names: Vec<&str> = events.iter().map(|e| e.event_name()).collect();
        assert!(event_names.contains(&"message_start"));
        assert!(event_names.contains(&"message_delta"));
        assert!(event_names.contains(&"message_stop"));
    }
}
