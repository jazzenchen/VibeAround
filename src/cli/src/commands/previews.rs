use va_client::http::AuthRequirement;
use va_client::ops;

use super::{print_json, run_unit, transport_for};
use crate::args::Options;
use crate::error::CliError;

pub(super) async fn list(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    if options.json {
        print_json(transport.execute_json(ops::previews()).await?)?;
        return Ok(());
    }
    let previews = transport.execute(ops::previews()).await?;
    println!("tunnel: {}", previews.tunnel_url.as_deref().unwrap_or("-"));
    for preview in previews.previews {
        println!("{}\t{:?}\t{}", preview.slug, preview.kind, preview.title);
    }
    Ok(())
}

pub(super) async fn delete(options: &Options, slug: &str) -> Result<(), CliError> {
    run_unit(options, ops::preview_delete(slug), "preview deleted").await
}
