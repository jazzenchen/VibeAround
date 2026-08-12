//! Owner Preview data consumed by the Web SPA.

use common::previews::PreviewSnapshot;
use serde::Serialize;

use super::iframe::preview_src;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OwnerPreviewBootstrap {
    selected_slug: String,
    previews: Vec<OwnerPreviewItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnerPreviewItem {
    slug: String,
    title: String,
    workspace: String,
    kind: &'static str,
    src: String,
    chat_available: bool,
}

pub(super) fn owner_preview_bootstrap(
    selected_slug: &str,
    previews: &[PreviewSnapshot],
    server_host: Option<&str>,
) -> OwnerPreviewBootstrap {
    let mut previews = previews.iter().collect::<Vec<_>>();
    previews.sort_by(|a, b| {
        a.workspace
            .cmp(&b.workspace)
            .then_with(|| a.created_at_ms.cmp(&b.created_at_ms))
    });

    OwnerPreviewBootstrap {
        selected_slug: selected_slug.to_string(),
        previews: previews
            .into_iter()
            .map(|preview| OwnerPreviewItem {
                slug: preview.slug.clone(),
                title: preview.title.clone(),
                workspace: preview.workspace.display().to_string(),
                kind: preview.kind,
                src: preview_src(&preview.slug, preview.port, server_host),
                // Chat is an owner Preview capability. The socket resolves an
                // existing child by slug or creates a standalone child when
                // no prior conversation exists.
                chat_available: true,
            })
            .collect(),
    }
}

#[cfg(test)]
#[path = "bootstrap_tests.rs"]
mod tests;
