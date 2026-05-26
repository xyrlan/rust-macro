use std::path::PathBuf;

use clap::{Parser, Subcommand};
use rm_error::Result;
use tracing_subscriber::EnvFilter;

mod commands;
mod stdio_driver;

#[derive(Parser)]
#[command(name = "macro-cli", version)]
struct Cli {
    /// Storage root. Defaults to `<data_dir>/rust-macro` (e.g. on Windows,
    /// `%APPDATA%/rust-macro`). Matches what the Tauri app will use in Plan 3.
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Record events from stdin (JSONL) and save under `name`.
    Record { name: String },
    /// Play the macro named `name` (events emitted to stdout JSONL).
    Play { name: String },
    /// List all saved macros.
    List,
    /// Delete the macro named `name`.
    Delete { name: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(|| {
        dirs::data_dir()
            .map(|d| d.join("rust-macro"))
            .unwrap_or_else(|| PathBuf::from("./.rust-macro"))
    });

    let res: Result<()> = match cli.cmd {
        Cmd::Record { name } => commands::cmd_record(&root, &name).await,
        Cmd::Play { name } => commands::cmd_play(&root, &name).await,
        Cmd::List => commands::cmd_list(&root),
        Cmd::Delete { name } => commands::cmd_delete(&root, &name),
    };
    res.map_err(|e| anyhow::anyhow!("{e}"))
}
