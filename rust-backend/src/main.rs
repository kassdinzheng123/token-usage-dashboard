use std::{env, error::Error, io, path::PathBuf};

use token_usage_server::{
    cleanup::{run_cleanup, CleanupMode},
    daily,
    ledger::UsageLedger,
    protocol::DailyGenerateRequest,
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
        Some("daily") => {
            handle_daily(args.collect()).await?;
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

async fn handle_daily(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err(invalid_input("daily requires projects or generate").into());
    };
    match action {
        "projects" => handle_daily_projects(&args[1..]),
        "generate" => handle_daily_generate(&args[1..]).await,
        _ => Err(invalid_input(format!(
            "unknown daily action: {action}; expected projects or generate"
        ))
        .into()),
    }
}

fn handle_daily_projects(args: &[String]) -> Result<(), Box<dyn Error>> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err(invalid_input("daily projects requires list, add, or remove").into());
    };
    match action {
        "list" => {
            let projects = daily::projects::list_projects().map_err(invalid_input)?;
            if projects.is_empty() {
                println!("no projects bound");
                return Ok(());
            }
            for binding in projects {
                println!("{}\t{}", binding.name, binding.path);
            }
        }
        "add" => {
            let (name, path) = parse_name_path(&args[1..])?;
            let binding =
                daily::projects::upsert_project(&name, &path).map_err(invalid_input)?;
            println!("added: {} -> {}", binding.name, binding.path);
        }
        "remove" => {
            let name = flag_value(&args[1..], "--name")?;
            let remaining =
                daily::projects::remove_project(&name).map_err(invalid_input)?;
            println!("removed: {name}");
            println!("remaining: {}", remaining.len());
        }
        _ => {
            return Err(invalid_input(format!(
                "unknown daily projects action: {action}; expected list, add, or remove"
            ))
            .into());
        }
    }
    Ok(())
}

async fn handle_daily_generate(args: &[String]) -> Result<(), Box<dyn Error>> {
    let mut project = None;
    let mut date = None;
    let mut force = false;
    let mut api_key = None;
    let mut model_url = None;
    let mut model_id = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--project" if project.is_none() => {
                project = Some(
                    args.get(index + 1)
                        .ok_or_else(|| invalid_input("--project requires a value"))?
                        .clone(),
                );
                index += 2;
            }
            "--date" if date.is_none() => {
                date = Some(
                    args.get(index + 1)
                        .ok_or_else(|| invalid_input("--date requires a value"))?
                        .clone(),
                );
                index += 2;
            }
            "--force" if !force => {
                force = true;
                index += 1;
            }
            "--api-key" if api_key.is_none() => {
                api_key = Some(
                    args.get(index + 1)
                        .ok_or_else(|| invalid_input("--api-key requires a value"))?
                        .clone(),
                );
                index += 2;
            }
            "--model-url" if model_url.is_none() => {
                model_url = Some(
                    args.get(index + 1)
                        .ok_or_else(|| invalid_input("--model-url requires a value"))?
                        .clone(),
                );
                index += 2;
            }
            "--model-id" if model_id.is_none() => {
                model_id = Some(
                    args.get(index + 1)
                        .ok_or_else(|| invalid_input("--model-id requires a value"))?
                        .clone(),
                );
                index += 2;
            }
            "--project" | "--date" | "--force" | "--api-key" | "--model-url" | "--model-id" => {
                return Err(invalid_input(format!("duplicate daily option: {}", args[index])).into());
            }
            other => {
                return Err(invalid_input(format!("unknown daily option: {other}")).into());
            }
        }
    }
    let project = project.ok_or_else(|| invalid_input("daily generate requires --project <name>"))?;
    let api_key = api_key.or_else(|| {
        env::var("TOKEN_USAGE_BRIEF_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
    });
    let report = tokio::task::spawn_blocking(move || {
        daily::generate_daily_report(DailyGenerateRequest {
            project,
            date,
            force: Some(force),
            model: Some(token_usage_server::protocol::BriefModelConfig {
                base_url: model_url.unwrap_or_else(|| "http://127.0.0.1:8317/v1".into()),
                api_key,
                model_id: model_id.unwrap_or_else(|| "deepseek-v4-flash".into()),
            }),
        })
    })
    .await
    .map_err(|err| invalid_input(err.to_string()))?
    .map_err(invalid_input)?;

    println!("daily report: {} / {}", report.project, report.date);
    println!("status: {}", report.status);
    if let Some(error) = &report.error {
        println!("error: {error}");
    }
    if !report.overview.is_empty() {
        println!("overview: {}", report.overview);
    }
    for (index, item) in report.work_items.iter().enumerate() {
        println!("{}: {}", index + 1, item.title);
        if !item.detail.is_empty() {
            println!("   {}", item.detail);
        }
    }
    println!(
        "stats: {} sessions, {} commits, {} tokens, coverage={}",
        report.session_count, report.commit_count, report.token_total, report.coverage
    );
    Ok(())
}

fn parse_name_path(args: &[String]) -> Result<(String, String), io::Error> {
    let name = flag_value(args, "--name")?;
    let path = flag_value(args, "--path")?;
    Ok((name, path))
}

fn flag_value(args: &[String], flag: &str) -> Result<String, io::Error> {
    let mut found = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == flag {
            if found.is_some() {
                return Err(invalid_input(format!("duplicate option: {flag}")));
            }
            found = Some(
                args.get(index + 1)
                    .ok_or_else(|| invalid_input(format!("{flag} requires a value")))?
                    .clone(),
            );
            index += 2;
        } else {
            index += 1;
        }
    }
    found.ok_or_else(|| invalid_input(format!("missing option: {flag}")))
}
