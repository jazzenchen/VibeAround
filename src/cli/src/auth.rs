use va_client::http::AuthRequirement;

use crate::args::Options;
use crate::config::{auth_file_path, remove_auth_file, resolve_endpoint_env, RuntimeEnv};
use crate::error::CliError;

pub(crate) fn status(options: &Options) -> Result<(), CliError> {
    let runtime_env = RuntimeEnv::current();
    let path = auth_file_path(options);
    let resolved = resolve_endpoint_env(options, AuthRequirement::BearerToken, &runtime_env);

    if options.json {
        match resolved {
            Ok(endpoint) => {
                crate::print_json(serde_json::json!({
                    "configured": true,
                    "endpoint": endpoint.endpoint.base_url(),
                    "base_url_source": endpoint.base_url_source,
                    "auth_source": endpoint.auth_source,
                    "auth_file": endpoint.auth_file.as_ref().map(|path| path.display().to_string()),
                    "resolved_auth_file": path.display().to_string()
                }))?;
            }
            Err(CliError::MissingAuth(_)) | Err(CliError::MissingToken) => {
                crate::print_json(serde_json::json!({
                    "configured": false,
                    "auth_file": path.display().to_string()
                }))?;
            }
            Err(error) => return Err(error),
        }
        return Ok(());
    }

    println!("auth file: {}", path.display());
    match resolved {
        Ok(endpoint) => {
            println!("configured: yes");
            println!("endpoint: {}", endpoint.endpoint.base_url());
            println!("base url source: {}", endpoint.base_url_source);
            println!("auth source: {}", endpoint.auth_source);
        }
        Err(CliError::MissingAuth(_)) | Err(CliError::MissingToken) => {
            println!("configured: no");
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

pub(crate) fn clear(options: &Options) -> Result<(), CliError> {
    let path = auth_file_path(options);
    let removed = remove_auth_file(options)?;
    if options.json {
        crate::print_json(serde_json::json!({
            "removed": removed.is_some(),
            "auth_file": path.display().to_string()
        }))?;
        return Ok(());
    }
    match removed {
        Some(path) => println!("removed: {}", path.display()),
        None => println!("not found: {}", path.display()),
    }
    Ok(())
}
