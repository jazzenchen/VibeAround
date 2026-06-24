use va_client::http::AuthRequirement;
use va_client::ops;

use super::{print_json, run_unit, transport_for};
use crate::args::Options;
use crate::error::CliError;

pub(super) async fn list(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    if options.json {
        print_json(transport.execute_json(ops::workspaces()).await?)?;
        return Ok(());
    }
    let workspaces = transport.execute(ops::workspaces()).await?;
    println!("default: {}", workspaces.default_workspace);
    for workspace in workspaces.workspaces {
        let marker = if workspace.is_default {
            "default"
        } else if workspace.is_builtin {
            "builtin"
        } else {
            ""
        };
        println!("{}\t{}", workspace.path, marker);
    }
    Ok(())
}

pub(super) async fn add(options: &Options, path: &str) -> Result<(), CliError> {
    run_unit(options, ops::workspace_add(path)?, "workspace added").await
}

pub(super) async fn remove(options: &Options, path: &str) -> Result<(), CliError> {
    run_unit(options, ops::workspace_remove(path)?, "workspace removed").await
}

pub(super) async fn set_default(options: &Options, path: &str) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    if options.json {
        print_json(
            transport
                .execute_json(ops::workspace_set_default(path)?)
                .await?,
        )?;
        return Ok(());
    }
    let workspaces = transport.execute(ops::workspace_set_default(path)?).await?;
    println!("default: {}", workspaces.default_workspace);
    println!("workspaces: {}", workspaces.workspaces.len());
    Ok(())
}

pub(super) async fn create(options: &Options, name: &str) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    if options.json {
        print_json(transport.execute_json(ops::workspace_create(name)?).await?)?;
        return Ok(());
    }
    let response = transport.execute(ops::workspace_create(name)?).await?;
    println!("created: {}", response.workspace.path);
    println!("default: {}", response.default_workspace);
    println!("workspaces: {}", response.workspaces.len());
    Ok(())
}
