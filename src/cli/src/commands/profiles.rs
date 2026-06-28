use va_client::http::AuthRequirement;
use va_client::ops;

use super::{print_json, transport_for};
use crate::args::Options;
use crate::error::CliError;

pub(super) async fn list(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    if options.json {
        print_json(transport.execute_json(ops::model_profiles()).await?)?;
        return Ok(());
    }
    for profile in transport.execute(ops::model_profiles()).await? {
        println!(
            "{}\t{}\t{}\t{}",
            profile.id, profile.label, profile.provider, profile.provider_label
        );
    }
    Ok(())
}
