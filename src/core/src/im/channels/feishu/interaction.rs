//! Feishu interactive card builder.
//! Constructs card JSON (v2 schema) for send_interactive.
//! Reference: https://open.feishu.cn/document/common-capabilities/message-card/

use crate::im::transport::{ButtonStyle, InteractiveOption};

/// Build a Feishu interactive card JSON string from a prompt and button options.
/// Uses card v2 (schema 2.0) — buttons grouped by `group` field.
/// Same-group buttons are laid out horizontally via column_set; different groups separated by hr.
pub fn build_card(title: &str, prompt: &str, options: &[InteractiveOption]) -> String {
    let mut elements: Vec<serde_json::Value> = vec![
        serde_json::json!({ "tag": "markdown", "content": prompt }),
        serde_json::json!({ "tag": "hr" }),
    ];

    // Group buttons by group index
    let mut groups: Vec<(u8, Vec<&InteractiveOption>)> = Vec::new();
    for opt in options {
        if let Some(g) = groups.last_mut().filter(|(g, _)| *g == opt.group) {
            g.1.push(opt);
        } else {
            groups.push((opt.group, vec![opt]));
        }
    }

    for (i, (_group_id, group_opts)) in groups.iter().enumerate() {
        if i > 0 {
            elements.push(serde_json::json!({ "tag": "hr" }));
        }

        if group_opts.len() == 1 {
            // Single button: just place it directly
            let opt = group_opts[0];
            elements.push(make_button(opt));
        } else {
            // Multiple buttons: use column_set for horizontal layout
            let columns: Vec<serde_json::Value> = group_opts.iter().map(|opt| {
                serde_json::json!({
                    "tag": "column",
                    "width": "weighted",
                    "weight": 1,
                    "elements": [ make_button(opt) ]
                })
            }).collect();

            elements.push(serde_json::json!({
                "tag": "column_set",
                "flex_mode": "stretch",
                "background_style": "default",
                "columns": columns
            }));
        }
    }

    let card = serde_json::json!({
        "schema": "2.0",
        "config": { "update_multi": true },
        "header": {
            "title": { "tag": "plain_text", "content": title },
            "template": "blue"
        },
        "body": { "elements": elements }
    });

    card.to_string()
}

fn make_button(opt: &InteractiveOption) -> serde_json::Value {
    let btn_type = match opt.style {
        ButtonStyle::Primary => "primary",
        ButtonStyle::Danger => "danger",
        ButtonStyle::Default => "default",
    };
    serde_json::json!({
        "tag": "button",
        "text": { "tag": "plain_text", "content": opt.label },
        "type": btn_type,
        "value": { "action": opt.value }
    })
}
