use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use enozunu::MANIFEST_FILE_NAME;
use enozunu::diagnostics::Diagnostic;
use enozunu::git::CommandGitResolver;

#[derive(Parser)]
#[command(
    name = "enozunu",
    version,
    about = "Cross-provider configuration materializer for AI agent tooling"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a starter manifest with placeholder values, without overwriting an existing one.
    Init {
        /// Path of the manifest to create. Defaults to enozunu.kdl in the project root.
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Project root directory. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project_root: PathBuf,
    },
    /// Parse and validate the manifest without materializing anything.
    Validate {
        /// Path to the manifest. Defaults to enozunu.kdl in the project root.
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Project root directory. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project_root: PathBuf,
    },
    /// Resolve sources and materialize them into target AI-native paths.
    // `summon` is the user-facing name for the internal materialization pipeline (`run_materialize`).
    // The CLI verb and the `materialize` module deliberately differ; keep both in mind when renaming either side.
    Summon {
        /// Path to the manifest. Defaults to enozunu.kdl in the project root.
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Project root directory. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project_root: PathBuf,
        /// Re-resolve branch and tag selectors and refresh enozunu.lock.json.
        #[arg(long, conflicts_with = "frozen")]
        update: bool,
        /// Fail unless every branch and tag selector is recorded in enozunu.lock.json; never write the lock file. Intended for CI.
        #[arg(long)]
        frozen: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Init {
            manifest,
            project_root,
        } => {
            let manifest_path = manifest.unwrap_or_else(|| project_root.join(MANIFEST_FILE_NAME));
            enozunu::init::run_init(&manifest_path, &project_root)
                .map(|()| {
                    println!("created {}", manifest_path.display());
                })
                .map_err(|d| vec![d])
        }
        Command::Validate {
            manifest,
            project_root,
        } => {
            let manifest_path = manifest.unwrap_or_else(|| project_root.join(MANIFEST_FILE_NAME));
            enozunu::load_manifest(&manifest_path).map(|_| {
                println!("{} is valid", manifest_path.display());
            })
        }
        Command::Summon {
            manifest,
            project_root,
            update,
            frozen,
        } => {
            let manifest_path = manifest.unwrap_or_else(|| project_root.join(MANIFEST_FILE_NAME));
            let resolver = CommandGitResolver::new(project_root.join(".enozunu/cache"));
            // `conflicts_with` already rejects `--update --frozen`, so the two flags collapse to one mode here.
            let lock_mode = if update {
                enozunu::LockMode::Update
            } else if frozen {
                enozunu::LockMode::Frozen
            } else {
                enozunu::LockMode::Locked
            };
            enozunu::run_materialize(&manifest_path, &project_root, &resolver, lock_mode).map(
                |outcome| {
                    for entry in &outcome.entries {
                        // The target AI is named explicitly: a Codex Skill materializes to `.agents/skills/`, whose prefix does not itself read as "codex".
                        println!(
                            "materialized {} `{}` for {} -> {} ({})",
                            entry.kind.as_str(),
                            entry.source_name,
                            entry.target_ai.as_str(),
                            entry.target_rel_path,
                            entry.origin.describe()
                        );
                    }
                    println!(
                        "recorded provenance in {}",
                        enozunu::provenance::PROVENANCE_REL_PATH
                    );
                    // Only a real file change is announced: an unchanged lock and frozen mode
                    // print nothing, so every lock line in the output means the file on disk moved.
                    // The default project root `.` would render as `./enozunu.lock.json`; stripping
                    // it matches how the provenance line prints a bare relative path.
                    let lock_path = outcome
                        .lock_path
                        .strip_prefix(".")
                        .unwrap_or(&outcome.lock_path);
                    match outcome.lock {
                        enozunu::LockOutcome::Created => {
                            println!("created {}", lock_path.display());
                        }
                        enozunu::LockOutcome::Updated => {
                            println!("updated {}", lock_path.display());
                        }
                        enozunu::LockOutcome::Unchanged | enozunu::LockOutcome::NotWritten => {}
                    }
                },
            )
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(diags) => {
            report(&diags);
            ExitCode::FAILURE
        }
    }
}

fn report(diags: &[Diagnostic]) {
    for diag in diags {
        eprintln!("{diag}");
    }
}
