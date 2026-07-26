use std::{env, error::Error, io, path::PathBuf};

use token_usage_server::{
    cleanup::{run_cleanup, CleanupMode},
    ledger::UsageLedger,
    sync::{export_to_git_repo, import_from_git_repo, sync_with_git},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None => {
            token_usage_server::server::serve_from_env().await?;
        }
        Some("cleanup") => {
            handle_cleanup(args.collect())?;
        }
        Some("sync") => {
            handle_sync(args.collect())?;
        }
        Some(other) => {
            return Err(invalid_input(format!("unknown command: {other}")).into());
        }
    }

    Ok(())
}

fn handle_cleanup(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    let mode = match args.as_slice() {
        [flag] if flag == "--dry-run" => CleanupMode::DryRun,
        [flag] if flag == "--apply" => CleanupMode::Apply,
        [] => {
            return Err(invalid_input("cleanup requires either --dry-run or --apply").into());
        }
        _ => {
            return Err(invalid_input(format!(
                "unknown arguments for cleanup: {}",
                args.join(" ")
            ))
            .into());
        }
    };

    let actions = run_cleanup(mode).map_err(invalid_input)?;
    let mode_label = match mode {
        CleanupMode::DryRun => "dry-run",
        CleanupMode::Apply => "apply",
    };

    println!("cleanup mode: {mode_label}");
    println!("actions: {}", actions.len());
    for action in actions {
        println!("{}", action.path.display());
    }

    Ok(())
}

fn handle_sync(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err(invalid_input("sync requires export, import, or run").into());
    };
    let mut repository = None;
    let mut device_id = None;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| invalid_input(format!("{flag} requires a value")))?;
        match flag {
            "--repo" if repository.is_none() => repository = Some(PathBuf::from(value)),
            "--device" if device_id.is_none() => device_id = Some(value.clone()),
            "--repo" | "--device" => {
                return Err(invalid_input(format!("duplicate sync option: {flag}")).into());
            }
            _ => return Err(invalid_input(format!("unknown sync option: {flag}")).into()),
        }
        index += 2;
    }
    let repository =
        repository.ok_or_else(|| invalid_input("sync requires --repo <git-working-tree>"))?;
    let ledger = UsageLedger::default().map_err(invalid_input)?;

    match action {
        "export" => {
            let device_id = device_id
                .or_else(|| env::var("TOKEN_USAGE_SYNC_DEVICE_ID").ok())
                .ok_or_else(|| {
                    invalid_input(
                        "sync export requires --device <id> or TOKEN_USAGE_SYNC_DEVICE_ID",
                    )
                })?;
            let (path, summary) =
                export_to_git_repo(&ledger, &repository, &device_id).map_err(invalid_input)?;
            println!("snapshot: {}", path.display());
            println!(
                "exported: {} records ({} sessions, {} blocks, {} messages)",
                summary.records(),
                summary.sessions,
                summary.blocks,
                summary.messages
            );
        }
        "import" => {
            if device_id.is_some() {
                return Err(invalid_input("--device is only valid for sync export or run").into());
            }
            let summary = import_from_git_repo(&ledger, &repository).map_err(invalid_input)?;
            println!(
                "imported: {} records from {} snapshots ({} sessions, {} blocks, {} messages)",
                summary.records(),
                summary.files,
                summary.sessions,
                summary.blocks,
                summary.messages
            );
        }
        "run" => {
            let device_id = device_id
                .or_else(|| env::var("TOKEN_USAGE_SYNC_DEVICE_ID").ok())
                .ok_or_else(|| {
                    invalid_input("sync run requires --device <id> or TOKEN_USAGE_SYNC_DEVICE_ID")
                })?;
            let result = sync_with_git(&ledger, &repository, &device_id).map_err(invalid_input)?;
            println!(
                "synced: {} imported, {} exported, committed={}, pushed={}, attempts={}",
                result.imported.records(),
                result.exported.records(),
                result.committed,
                result.pushed,
                result.attempts
            );
            if let Some(commit) = result.commit {
                println!("commit: {commit}");
            }
        }
        _ => {
            return Err(invalid_input(format!(
                "unknown sync action: {action}; expected export, import, or run"
            ))
            .into());
        }
    }

    Ok(())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
