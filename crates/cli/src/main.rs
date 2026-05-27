use std::path::PathBuf;

use clap::{Parser, Subcommand};
use rm_error::Result;
use tracing_subscriber::EnvFilter;

mod commands;
mod stdio_driver;

use commands::DriverKind;

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
    /// Record events from the selected driver and save under `name`.
    Record {
        name: String,
        #[cfg(feature = "interception")]
        #[arg(long, value_enum, default_value_t = DriverKindArg::Stdio)]
        driver: DriverKindArg,
    },
    /// Play the macro named `name` via the selected driver.
    Play {
        name: String,
        #[cfg(feature = "interception")]
        #[arg(long, value_enum, default_value_t = DriverKindArg::Stdio)]
        driver: DriverKindArg,
    },
    /// List all saved macros.
    List,
    /// Delete the macro named `name`.
    Delete { name: String },
    /// Interception driver utilities (status / install instructions).
    #[cfg(feature = "interception")]
    Driver {
        #[command(subcommand)]
        sub: DriverCmd,
    },
}

#[cfg(feature = "interception")]
#[derive(Subcommand)]
enum DriverCmd {
    /// Print Interception driver status.
    Status,
}

#[cfg(feature = "interception")]
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum DriverKindArg {
    Stdio,
    Interception,
}

#[cfg(feature = "interception")]
impl From<DriverKindArg> for DriverKind {
    fn from(d: DriverKindArg) -> Self {
        match d {
            DriverKindArg::Stdio => DriverKind::Stdio,
            DriverKindArg::Interception => DriverKind::Interception,
        }
    }
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
        Cmd::Record {
            name,
            #[cfg(feature = "interception")]
            driver,
        } => {
            #[cfg(feature = "interception")]
            let kind = driver.into();
            #[cfg(not(feature = "interception"))]
            let kind = DriverKind::Stdio;
            commands::cmd_record(&root, &name, kind).await
        }
        Cmd::Play {
            name,
            #[cfg(feature = "interception")]
            driver,
        } => {
            #[cfg(feature = "interception")]
            let kind = driver.into();
            #[cfg(not(feature = "interception"))]
            let kind = DriverKind::Stdio;
            commands::cmd_play(&root, &name, kind).await
        }
        Cmd::List => commands::cmd_list(&root),
        Cmd::Delete { name } => commands::cmd_delete(&root, &name),
        #[cfg(feature = "interception")]
        Cmd::Driver { sub } => match sub {
            DriverCmd::Status => commands::cmd_driver_status(),
        },
    };
    res.map_err(|e| anyhow::anyhow!("{e}"))
}
