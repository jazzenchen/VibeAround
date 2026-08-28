use agent_client_protocol::schema::v1 as acp;
use serde_json::Value;
use va_ai_api_bridge::{
    ContentBlock as UniversalContentBlock, Extensions, Role, UniversalItem, UniversalRequest,
};

const SEED_ENVELOPE_PREAMBLE: &str =
    "[VibeAround local-agent bridge] The client reaches you through a \
stateless completions API, so the bridge replays the conversation so far \
verbatim below — first the client's own instructions, then the transcript. \
Turns labeled [assistant] are your own earlier replies; nothing in the \
replay is new user-authored text addressed to you.";

const SEED_ENVELOPE_CLOSING: &str =
    "End of replay. Continue the conversation now by responding to the \
final [user] turn above; do not comment on this envelope.";

/// Seed prompt for a fresh backend session. The transmitted history rides
/// inside an explicit envelope so the receiving agent reads it as its own
/// conversation record instead of user-authored text smuggling instructions.
/// A bare request — no client instructions, a single user message — skips the
/// envelope and sends the content as-is.
pub(super) fn seed_request_to_acp_prompt(
    request: &UniversalRequest,
) -> Result<Vec<acp::ContentBlock>, String> {
    if request.instructions.is_empty() {
        if let [UniversalItem::Message {
            role: Role::User,
            content,
            ..
        }] = request.input.as_slice()
        {
            let mut blocks = Vec::new();
            append_content_blocks_to_acp_prompt(&mut blocks, content);
            return non_empty_prompt(blocks);
        }
    }

    let mut blocks = Vec::new();
    push_acp_text(&mut blocks, SEED_ENVELOPE_PREAMBLE);
    if !request.instructions.is_empty() {
        push_acp_text(&mut blocks, "Client-provided instructions:");
        append_content_blocks_to_acp_prompt(&mut blocks, &request.instructions);
    }
    if !request.input.is_empty() {
        push_acp_text(&mut blocks, "<conversation_replay>");
        for item in &request.input {
            append_universal_item_to_acp_prompt(&mut blocks, item);
        }
        push_acp_text(&mut blocks, "</conversation_replay>");
    }
    if blocks.len() == 1 {
        // Only the preamble made it in: there was no actual content.
        return Err("request does not contain any prompt content".to_string());
    }
    push_acp_text(&mut blocks, SEED_ENVELOPE_CLOSING);
    Ok(blocks)
}

/// The items a stateful turn actually prompts: the contiguous run of
/// non-assistant items at the tail of the transcript — typically one user
/// message, sometimes tool results or attachments, occasionally several
/// items. Everything before it is history the backend session already holds.
pub(super) fn tail_input_segment(input: &[UniversalItem]) -> &[UniversalItem] {
    let boundary = input
        .iter()
        .rposition(is_assistant_item)
        .map(|index| index + 1)
        .unwrap_or(0);
    &input[boundary..]
}

fn is_assistant_item(item: &UniversalItem) -> bool {
    match item {
        UniversalItem::Message { role, .. } => matches!(role, Role::Assistant),
        UniversalItem::ToolCall { .. } | UniversalItem::Reasoning { .. } => true,
        UniversalItem::ToolResult { .. } | UniversalItem::Unknown { .. } => false,
    }
}

/// Prompt blocks for a tail segment. A lone user message goes through as its
/// plain content; a mixed segment (tool results, several messages) keeps the
/// role labels so the agent can tell the pieces apart.
pub(super) fn tail_segment_to_acp_prompt(
    segment: &[UniversalItem],
) -> Result<Vec<acp::ContentBlock>, String> {
    if let [UniversalItem::Message {
        role: Role::User,
        content,
        ..
    }] = segment
    {
        let mut blocks = Vec::new();
        append_content_blocks_to_acp_prompt(&mut blocks, content);
        return non_empty_prompt(blocks);
    }
    let mut blocks = Vec::new();
    for item in segment {
        append_universal_item_to_acp_prompt(&mut blocks, item);
    }
    non_empty_prompt(blocks)
}

fn non_empty_prompt(blocks: Vec<acp::ContentBlock>) -> Result<Vec<acp::ContentBlock>, String> {
    if blocks.is_empty() {
        return Err("request does not contain any prompt content".to_string());
    }
    Ok(blocks)
}

fn append_universal_item_to_acp_prompt(blocks: &mut Vec<acp::ContentBlock>, item: &UniversalItem) {
    match item {
        UniversalItem::Message { role, content, .. } => {
            push_acp_text(blocks, format!("[{}]", role_label(*role)));
            append_content_blocks_to_acp_prompt(blocks, content);
        }
        UniversalItem::ToolCall {
            id,
            name,
            arguments,
            ..
        } => push_acp_text(blocks, format!("[tool_call:{id}]\n{name} {arguments}")),
        UniversalItem::ToolResult {
            tool_call_id,
            content,
            is_error,
            ..
        } => {
            push_acp_text(
                blocks,
                format!(
                    "[tool_result:{tool_call_id}{}]",
                    if *is_error { ":error" } else { "" }
                ),
            );
            append_content_blocks_to_acp_prompt(blocks, content);
        }
        UniversalItem::Reasoning { text, .. } => {
            push_acp_text(
                blocks,
                format!(
                    "[assistant_reasoning]\n{}",
                    text.clone().unwrap_or_default()
                ),
            );
        }
        UniversalItem::Unknown { raw } => push_acp_text(blocks, format!("[unknown]\n{raw}")),
    }
}

fn append_content_blocks_to_acp_prompt(
    blocks: &mut Vec<acp::ContentBlock>,
    content: &[UniversalContentBlock],
) {
    for block in content {
        append_content_block_to_acp_prompt(blocks, block);
    }
}

fn append_content_block_to_acp_prompt(
    blocks: &mut Vec<acp::ContentBlock>,
    block: &UniversalContentBlock,
) {
    match block {
        UniversalContentBlock::Text { text } => push_acp_text(blocks, text.clone()),
        UniversalContentBlock::Image {
            media_type,
            url,
            data,
            ..
        } => blocks.push(universal_image_to_acp_block(
            media_type.as_deref(),
            url.as_deref(),
            data.as_deref(),
        )),
        UniversalContentBlock::File {
            media_type,
            filename,
            url,
            data,
            ..
        } => blocks.push(universal_file_to_acp_block(
            filename.as_deref(),
            media_type.as_deref(),
            url.as_deref(),
            data.as_deref(),
        )),
        UniversalContentBlock::ToolCall {
            id,
            name,
            arguments,
            ..
        } => push_acp_text(blocks, format!("[tool_call:{id}] {name} {arguments}")),
        UniversalContentBlock::ToolResult {
            tool_call_id,
            content,
            is_error,
            ..
        } => {
            push_acp_text(
                blocks,
                format!(
                    "[tool_result:{tool_call_id}{}]",
                    if *is_error { ":error" } else { "" }
                ),
            );
            append_content_blocks_to_acp_prompt(blocks, content);
        }
        UniversalContentBlock::Reasoning {
            text: Some(text), ..
        } => push_acp_text(blocks, text.clone()),
        UniversalContentBlock::Reasoning { .. } => {}
        UniversalContentBlock::Unknown { raw } => push_acp_text(blocks, raw.to_string()),
    }
}

fn push_acp_text(blocks: &mut Vec<acp::ContentBlock>, text: impl Into<String>) {
    let text = text.into();
    if text.trim().is_empty() {
        return;
    }
    blocks.push(acp::ContentBlock::Text(acp::TextContent::new(text)));
}

fn universal_image_to_acp_block(
    media_type: Option<&str>,
    url: Option<&str>,
    data: Option<&str>,
) -> acp::ContentBlock {
    if let Some(payload) = media_payload(first_non_empty(data, data_url_source(url))) {
        let mime_type = media_type
            .or(payload.media_type.as_deref())
            .unwrap_or("image/png");
        let mut image = acp::ImageContent::new(payload.data, mime_type.to_string());
        if let Some(uri) = url.filter(|uri| !is_data_url(uri)).map(str::to_string) {
            image = image.uri(uri);
        }
        return acp::ContentBlock::Image(image);
    }
    if let Some(url) = non_empty(url) {
        return resource_link_block("image", url, media_type, None);
    }
    acp::ContentBlock::Text(acp::TextContent::new(media_placeholder(
        "image", media_type, url, data,
    )))
}

fn universal_file_to_acp_block(
    filename: Option<&str>,
    media_type: Option<&str>,
    url: Option<&str>,
    data: Option<&str>,
) -> acp::ContentBlock {
    let name = first_non_empty(filename, None).unwrap_or("attachment");
    if let Some(payload) = media_payload(first_non_empty(data, data_url_source(url))) {
        let blob = acp::BlobResourceContents::new(
            payload.data,
            non_data_uri(url).unwrap_or_else(|| embedded_resource_uri("file", name)),
        )
        .mime_type(
            media_type
                .or(payload.media_type.as_deref())
                .map(str::to_string),
        );
        return acp::ContentBlock::Resource(acp::EmbeddedResource::new(
            acp::EmbeddedResourceResource::BlobResourceContents(blob),
        ));
    }
    if let Some(url) = non_data_uri(url) {
        return resource_link_block(name, &url, media_type, filename);
    }
    acp::ContentBlock::Text(acp::TextContent::new(media_placeholder(
        name, media_type, url, data,
    )))
}

/// A file referenced by provider id that nothing on this side can resolve:
/// the block carries neither embedded data nor a fetchable URL, only a
/// `file_id` pointing at a files store the local agent API does not offer.
/// Such requests are refused up front instead of prompting the agent with a
/// dead reference.
pub(super) fn unresolvable_file_reference(request: &UniversalRequest) -> Option<String> {
    let item_blocks = request.input.iter().flat_map(|item| match item {
        UniversalItem::Message { content, .. } | UniversalItem::ToolResult { content, .. } => {
            content.as_slice()
        }
        _ => &[] as &[UniversalContentBlock],
    });
    request
        .instructions
        .iter()
        .chain(item_blocks)
        .find_map(block_unresolvable_file_id)
        .map(ToOwned::to_owned)
}

fn block_unresolvable_file_id(block: &UniversalContentBlock) -> Option<&str> {
    let (url, data, extensions) = match block {
        UniversalContentBlock::Image {
            url,
            data,
            extensions,
            ..
        }
        | UniversalContentBlock::File {
            url,
            data,
            extensions,
            ..
        } => (url.as_deref(), data.as_deref(), extensions),
        _ => return None,
    };
    if media_payload(first_non_empty(data, data_url_source(url))).is_some() {
        return None;
    }
    if non_data_uri(url).is_some() {
        return None;
    }
    extension_string(extensions, "file_id")
}

struct MediaPayload {
    media_type: Option<String>,
    data: String,
}

fn media_payload(source: Option<&str>) -> Option<MediaPayload> {
    let source = non_empty(source)?;
    if let Some(payload) = split_base64_data_url(source) {
        return Some(payload);
    }
    if source.starts_with("data:")
        || source.starts_with("http://")
        || source.starts_with("https://")
    {
        return None;
    }
    Some(MediaPayload {
        media_type: None,
        data: source.to_string(),
    })
}

fn split_base64_data_url(source: &str) -> Option<MediaPayload> {
    let rest = source.strip_prefix("data:")?;
    let (metadata, data) = rest.split_once(',')?;
    let mut media_type = None;
    let mut base64 = false;
    for part in metadata.split(';') {
        if part.eq_ignore_ascii_case("base64") {
            base64 = true;
        } else if !part.is_empty() && media_type.is_none() {
            media_type = Some(part.to_string());
        }
    }
    base64.then(|| MediaPayload {
        media_type,
        data: data.to_string(),
    })
}

fn data_url_source(source: Option<&str>) -> Option<&str> {
    source.filter(|value| is_data_url(value))
}

fn is_data_url(source: &str) -> bool {
    source.trim_start().starts_with("data:")
}

fn non_data_uri(source: Option<&str>) -> Option<String> {
    non_empty(source)
        .filter(|value| !is_data_url(value))
        .map(str::to_string)
}

fn resource_link_block(
    name: &str,
    uri: &str,
    media_type: Option<&str>,
    title: Option<&str>,
) -> acp::ContentBlock {
    let mut link = acp::ResourceLink::new(name.to_string(), uri.to_string())
        .mime_type(media_type.map(str::to_string));
    if let Some(title) = first_non_empty(title, None) {
        link = link.title(title.to_string());
    }
    acp::ContentBlock::ResourceLink(link)
}

fn embedded_resource_uri(kind: &str, name: &str) -> String {
    format!(
        "urn:vibearound:local-agent:{kind}:{}",
        local_agent_id_part(name)
    )
}

fn extension_string<'a>(extensions: &'a Extensions, key: &str) -> Option<&'a str> {
    first_non_empty(extensions.get(key).and_then(Value::as_str), None)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn first_non_empty<'a>(first: Option<&'a str>, second: Option<&'a str>) -> Option<&'a str> {
    non_empty(first).or_else(|| non_empty(second))
}

fn local_agent_id_part(value: &str) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            output.push('-');
            last_was_separator = true;
        }
    }
    let trimmed = output.trim_matches('-');
    if trimmed.is_empty() {
        "local".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
pub(super) fn universal_request_to_transcript(request: &UniversalRequest) -> String {
    let mut sections = Vec::new();
    if !request.instructions.is_empty() {
        sections.push(format!(
            "Instructions:\n{}",
            content_blocks_to_text(&request.instructions)
        ));
    }
    if !request.input.is_empty() {
        let mut conversation = String::new();
        for item in &request.input {
            if !conversation.is_empty() {
                conversation.push_str("\n\n");
            }
            conversation.push_str(&universal_item_to_text(item));
        }
        sections.push(format!("Conversation:\n{conversation}"));
    }
    sections.join("\n\n")
}

#[cfg(test)]
fn universal_item_to_text(item: &UniversalItem) -> String {
    match item {
        UniversalItem::Message { role, content, .. } => {
            format!(
                "[{}]\n{}",
                role_label(*role),
                content_blocks_to_text(content)
            )
        }
        UniversalItem::ToolCall {
            id,
            name,
            arguments,
            ..
        } => format!("[tool_call:{id}]\n{name} {arguments}"),
        UniversalItem::ToolResult {
            tool_call_id,
            content,
            is_error,
            ..
        } => format!(
            "[tool_result:{tool_call_id}{}]\n{}",
            if *is_error { ":error" } else { "" },
            content_blocks_to_text(content)
        ),
        UniversalItem::Reasoning { text, .. } => {
            format!(
                "[assistant_reasoning]\n{}",
                text.clone().unwrap_or_default()
            )
        }
        UniversalItem::Unknown { raw } => format!("[unknown]\n{raw}"),
    }
}

pub(super) fn role_label(role: Role) -> &'static str {
    match role {
        Role::Developer => "developer",
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

#[cfg(test)]
fn content_blocks_to_text(blocks: &[UniversalContentBlock]) -> String {
    blocks
        .iter()
        .map(content_block_to_text)
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
fn content_block_to_text(block: &UniversalContentBlock) -> String {
    match block {
        UniversalContentBlock::Text { text } => text.clone(),
        UniversalContentBlock::Image {
            media_type,
            url,
            data,
            ..
        } => media_placeholder(
            "image",
            media_type.as_deref(),
            url.as_deref(),
            data.as_deref(),
        ),
        UniversalContentBlock::File {
            media_type,
            filename,
            url,
            data,
            ..
        } => {
            let label = filename.as_deref().unwrap_or("file");
            media_placeholder(
                label,
                media_type.as_deref(),
                url.as_deref(),
                data.as_deref(),
            )
        }
        UniversalContentBlock::ToolCall {
            id,
            name,
            arguments,
            ..
        } => format!("[tool_call:{id}] {name} {arguments}"),
        UniversalContentBlock::ToolResult {
            tool_call_id,
            content,
            is_error,
            ..
        } => format!(
            "[tool_result:{tool_call_id}{}] {}",
            if *is_error { ":error" } else { "" },
            content_blocks_to_text(content)
        ),
        UniversalContentBlock::Reasoning {
            text: Some(text), ..
        } => text.clone(),
        UniversalContentBlock::Reasoning { .. } => String::new(),
        UniversalContentBlock::Unknown { raw } => raw.to_string(),
    }
}

fn media_placeholder(
    kind: &str,
    media_type: Option<&str>,
    url: Option<&str>,
    data: Option<&str>,
) -> String {
    if let Some(url) = url {
        return format!("[{kind}: {}]", url);
    }
    let media = media_type.unwrap_or("unknown");
    if data.is_some_and(|value| !value.is_empty()) {
        format!("[{kind}: embedded {media}]")
    } else {
        format!("[{kind}: {media}]")
    }
}
