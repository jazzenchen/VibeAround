use agent_client_protocol::schema as acp;
use serde_json::Value;

pub(crate) const PROTOCOL_OPEN: &str = "<va-agent-protocol>";
pub(crate) const PROTOCOL_CLOSE: &str = "</va-agent-protocol>";

#[derive(Debug, Default)]
pub(crate) struct AgentProtocolFilter {
    state: ProtocolState,
}

#[derive(Debug, Default)]
enum ProtocolState {
    #[default]
    Normal,
    HoldingTail {
        tail: String,
    },
    InFrame {
        frame: String,
    },
    AfterFrame {
        frame: String,
    },
    Violated {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentProtocolFinish {
    pub visible_tail: String,
    pub frame: Option<Result<String, String>>,
}

impl AgentProtocolFilter {
    pub(crate) fn feed_text(&mut self, text: &str) -> String {
        match std::mem::take(&mut self.state) {
            ProtocolState::Normal => self.feed_normal("", text),
            ProtocolState::HoldingTail { tail } => self.feed_normal(&tail, text),
            ProtocolState::InFrame { mut frame } => self.feed_in_frame(&mut frame, text),
            ProtocolState::AfterFrame { frame } => self.feed_after_frame(frame, text),
            ProtocolState::Violated { reason } => {
                self.state = ProtocolState::Violated { reason };
                strip_protocol_segments(text)
            }
        }
    }

    pub(crate) fn finish(&mut self) -> AgentProtocolFinish {
        let state = std::mem::take(&mut self.state);
        match state {
            ProtocolState::Normal => AgentProtocolFinish {
                visible_tail: String::new(),
                frame: None,
            },
            ProtocolState::HoldingTail { tail } => AgentProtocolFinish {
                visible_tail: tail,
                frame: None,
            },
            ProtocolState::InFrame { .. } => AgentProtocolFinish {
                visible_tail: String::new(),
                frame: Some(Err(
                    "va-agent-protocol envelope was opened but not closed".to_string()
                )),
            },
            ProtocolState::AfterFrame { frame } => AgentProtocolFinish {
                visible_tail: String::new(),
                frame: Some(Ok(frame.trim().to_string())),
            },
            ProtocolState::Violated { reason } => AgentProtocolFinish {
                visible_tail: String::new(),
                frame: Some(Err(reason)),
            },
        }
    }

    fn feed_normal(&mut self, previous_tail: &str, text: &str) -> String {
        let combined = format!("{}{}", previous_tail, text);
        let Some(open_start) = combined.find(PROTOCOL_OPEN) else {
            let split_at = protocol_prefix_tail_start(&combined);
            let visible = combined[..split_at].to_string();
            let tail = combined[split_at..].to_string();
            self.state = if tail.is_empty() {
                ProtocolState::Normal
            } else {
                ProtocolState::HoldingTail { tail }
            };
            return visible;
        };

        let visible = combined[..open_start].to_string();
        let rest = &combined[open_start + PROTOCOL_OPEN.len()..];
        self.state = ProtocolState::InFrame {
            frame: String::new(),
        };
        let frame_visible = self.feed_text(rest);
        format!("{}{}", visible, frame_visible)
    }

    fn feed_in_frame(&mut self, frame: &mut String, text: &str) -> String {
        if let Some(close_start) = text.find(PROTOCOL_CLOSE) {
            frame.push_str(&text[..close_start]);
            let after_close = &text[close_start + PROTOCOL_CLOSE.len()..];
            let frame = std::mem::take(frame);
            self.state = ProtocolState::AfterFrame { frame };
            return self.feed_text(after_close);
        }

        frame.push_str(text);
        self.state = ProtocolState::InFrame {
            frame: std::mem::take(frame),
        };
        String::new()
    }

    fn feed_after_frame(&mut self, frame: String, text: &str) -> String {
        if text.trim().is_empty() {
            self.state = ProtocolState::AfterFrame { frame };
            return String::new();
        }
        self.state = ProtocolState::Violated {
            reason: "va-agent-protocol envelope must be the final content in the assistant message"
                .to_string(),
        };
        strip_protocol_segments(text)
    }
}

pub(crate) fn session_update_text(update: &acp::SessionUpdate) -> Option<&str> {
    let acp::SessionUpdate::AgentMessageChunk(chunk) = update else {
        return None;
    };
    match &chunk.content {
        acp::ContentBlock::Text(text) => Some(text.text.as_str()),
        _ => None,
    }
}

pub(crate) fn notification_payload(args: &acp::SessionNotification) -> acp::Result<Value> {
    serde_json::to_value(args).map_err(|e| acp::Error::new(-32603, format!("serialize: {}", e)))
}

pub(crate) fn notification_payload_with_text(
    args: &acp::SessionNotification,
    text: String,
) -> acp::Result<Value> {
    let mut payload = notification_payload(args)?;
    if let Some(content) = payload
        .get_mut("update")
        .and_then(|update| update.get_mut("content"))
        .and_then(|content| content.as_object_mut())
    {
        content.insert("text".to_string(), Value::String(text));
    }
    Ok(payload)
}

pub(crate) fn synthetic_agent_message_payload(session_id: &str, text: String) -> Value {
    let notification = acp::SessionNotification::new(
        session_id.to_string(),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(text),
        ))),
    );
    serde_json::to_value(notification).unwrap_or_else(|_| serde_json::json!({}))
}

pub(crate) fn synthetic_user_message_payload(session_id: &str, text: String) -> Value {
    let notification = acp::SessionNotification::new(
        session_id.to_string(),
        acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(text),
        ))),
    );
    serde_json::to_value(notification).unwrap_or_else(|_| serde_json::json!({}))
}

fn protocol_prefix_tail_start(text: &str) -> usize {
    for (idx, _) in text.char_indices() {
        let suffix = &text[idx..];
        if suffix.len() < PROTOCOL_OPEN.len() && PROTOCOL_OPEN.starts_with(suffix) {
            return idx;
        }
    }
    text.len()
}

fn strip_protocol_segments(input: &str) -> String {
    let mut output = String::new();
    let mut rest = input;
    loop {
        let Some(open_start) = rest.find(PROTOCOL_OPEN) else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..open_start]);
        let after_open = &rest[open_start + PROTOCOL_OPEN.len()..];
        let Some(close_start) = after_open.find(PROTOCOL_CLOSE) else {
            break;
        };
        rest = &after_open[close_start + PROTOCOL_CLOSE.len()..];
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_visible_text_and_hides_protocol_frame() {
        let mut filter = AgentProtocolFilter::default();

        assert_eq!(filter.feed_text("visible "), "visible ");
        assert_eq!(filter.feed_text("text<va-agent-protocol>{"), "text");
        assert_eq!(filter.feed_text("\"kind\":\"assignment\"}"), "");
        assert_eq!(filter.feed_text("</va-agent-protocol>   "), "");

        let finished = filter.finish();
        assert_eq!(finished.visible_tail, "");
        assert_eq!(
            finished.frame.unwrap().unwrap(),
            "{\"kind\":\"assignment\"}"
        );
    }

    #[test]
    fn detects_split_open_tag_without_exposing_it() {
        let mut filter = AgentProtocolFilter::default();

        assert_eq!(filter.feed_text("abc <va-agent-pro"), "abc ");
        assert_eq!(filter.feed_text("tocol>{}</va-agent-protocol>"), "");

        assert_eq!(filter.finish().frame.unwrap().unwrap(), "{}");
    }

    #[test]
    fn rejects_content_after_protocol_frame() {
        let mut filter = AgentProtocolFilter::default();

        assert_eq!(
            filter.feed_text("<va-agent-protocol>{}</va-agent-protocol> after"),
            " after"
        );
        let err = filter.finish().frame.unwrap().unwrap_err();

        assert!(err.contains("final content"));
    }

    #[test]
    fn flushes_tail_when_no_frame_was_seen() {
        let mut filter = AgentProtocolFilter::default();

        assert_eq!(filter.feed_text("hello world"), "hello world");
        let finished = filter.finish();

        assert_eq!(finished.visible_tail, "");
        assert!(finished.frame.is_none());
    }
}
