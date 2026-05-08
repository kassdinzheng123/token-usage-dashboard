use std::{env, error::Error, io};

use token_usage_server::cleanup::{run_cleanup, CleanupMode};

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

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
