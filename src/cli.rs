use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "kds")]
#[command(version)]
#[command(about = "KD Savings: compact command evidence and local log drilldown")]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run a command through KDS explicitly.
    Run(WrappedCommand),
    /// Tee stdout/stderr live through KDS.
    Raw(WrappedCommand),
    /// Summarize stdin or an existing log file.
    Summarize(SummarizeArgs),
    /// Show estimated output reduction metrics.
    Gain,
    /// Remove old local KDS run artifacts.
    Gc(GcArgs),
    /// Prune old local KDS run artifacts.
    Prune(PruneArgs),
    /// Run read-only health checks.
    Doctor,
    /// Inspect stored log metadata and safe sections.
    Logs(LogsArgs),
    /// Print a tiny Codex handoff bundle.
    Evidence(EvidenceArgs),
    /// Manage Codex guidance.
    Init(InitArgs),
    /// Manage automatic shell hooks.
    Hook(HookArgs),
}

#[derive(Debug, Args)]
pub struct WrappedCommand {
    #[arg(long)]
    pub show_paths: bool,
    #[arg(long, value_enum)]
    pub budget: Option<SummaryBudget>,
    /// Persist local KDS artifacts for later logs/evidence/gain commands.
    #[arg(long = "save-artifacts")]
    pub save_artifacts: bool,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
pub struct SummarizeArgs {
    /// Read log text from this file instead of stdin.
    #[arg(short, long)]
    pub file: Option<PathBuf>,
    /// Stable label for the imported log in KDS metadata.
    #[arg(long)]
    pub name: Option<String>,
    /// Exit code to record for the imported output.
    #[arg(long = "exit-code", default_value_t = 1)]
    pub exit_code: i32,
    #[arg(long)]
    pub show_paths: bool,
    #[arg(long, value_enum)]
    pub budget: Option<SummaryBudget>,
    /// Persist local KDS artifacts for later logs/evidence/gain commands.
    #[arg(long = "save-artifacts")]
    pub save_artifacts: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SummaryBudget {
    Tight,
    Normal,
    Wide,
}

#[derive(Debug, Args)]
pub struct LogsArgs {
    #[command(subcommand)]
    command: LogsCommand,
}

#[derive(Debug, Subcommand)]
pub enum LogsCommand {
    /// Print the KDS log directory.
    Dir,
    /// Print safe local log storage statistics.
    Stats,
    /// Print safe metadata for the most recent run.
    Last(LogsDisplayArgs),
    /// Show safe metadata or one requested section for a run.
    Show(LogsShowArgs),
}

#[derive(Debug, Args)]
pub struct LogsDisplayArgs {
    #[arg(long)]
    pub show_paths: bool,
}

#[derive(Debug, Args)]
pub struct LogsShowArgs {
    pub id: String,
    #[arg(long)]
    pub show_paths: bool,
    #[arg(long)]
    pub summary: bool,
    #[arg(long)]
    pub errors: bool,
    #[arg(long = "error-window")]
    pub error_window: bool,
    #[arg(long)]
    pub tail: bool,
    #[arg(long = "file-hits")]
    pub file_hits: bool,
}

#[derive(Debug, Args)]
pub struct GcArgs {
    /// Remove artifacts older than this age, such as 30d, 12h, or 90m.
    #[arg(long = "older-than")]
    pub older_than: String,
    /// Report what would be deleted without removing files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct PruneArgs {
    /// Remove artifacts older than this age, such as 30d, 12h, or 90m.
    #[arg(long = "before")]
    pub before: String,
    /// Report what would be deleted without removing files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct EvidenceArgs {
    pub id: String,
    #[arg(long)]
    pub show_paths: bool,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(short = 'g', long)]
    pub global: bool,
    #[arg(long)]
    pub codex: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub yes: bool,
    #[arg(long)]
    pub uninstall: bool,
}

#[derive(Debug, Args)]
pub struct HookArgs {
    #[command(subcommand)]
    pub command: HookCommand,
}

#[derive(Debug, Subcommand)]
pub enum HookCommand {
    /// Print hook install status.
    Status,
    /// Run read-only hook diagnostics.
    Doctor,
    /// Install or repair a shell hook.
    Install(HookShellArg),
    /// Remove a KDS-managed shell hook.
    Uninstall(HookShellArg),
}

#[derive(Debug, Args)]
pub struct HookShellArg {
    pub shell: HookShell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HookShell {
    Powershell,
}

pub fn run() -> Result<i32> {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    if raw_args.first().map(String::as_str) == Some("--") {
        return crate::runner::run(
            raw_args.into_iter().skip(1).collect(),
            crate::runner::Mode::Compact,
            false,
            None,
            false,
        );
    }

    let cli = Cli::parse();
    match cli.command {
        Some(Command::Run(args)) => crate::runner::run(
            args.command,
            crate::runner::Mode::Compact,
            args.show_paths,
            args.budget,
            args.save_artifacts,
        ),
        Some(Command::Raw(args)) => crate::runner::run(
            args.command,
            crate::runner::Mode::Raw,
            args.show_paths,
            args.budget,
            args.save_artifacts,
        ),
        Some(Command::Summarize(args)) => crate::runner::summarize_import(args),
        Some(Command::Gain) => crate::gain::run(),
        Some(Command::Gc(args)) => crate::gc::run(args),
        Some(Command::Prune(args)) => crate::gc::run_prune(args),
        Some(Command::Doctor) => crate::doctor::run(),
        Some(Command::Logs(args)) => crate::logs::run(args.command),
        Some(Command::Evidence(args)) => crate::evidence::run(args.id, args.show_paths),
        Some(Command::Init(args)) => crate::init_codex::run(args),
        Some(Command::Hook(args)) => crate::hook::run(args.command),
        None => bail!("no command provided; use `kds -- <command...>` or `kds --help`"),
    }
}
