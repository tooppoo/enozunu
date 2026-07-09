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
        } => {
            let manifest_path = manifest.unwrap_or_else(|| project_root.join(MANIFEST_FILE_NAME));
            let resolver = CommandGitResolver::new(project_root.join(".enozunu/cache"));
            enozunu::run_materialize(&manifest_path, &project_root, &resolver).map(|entries| {
                for entry in &entries {
                    let origin = match &entry.origin {
                        enozunu::ResolvedOrigin::Git { revision } => revision.clone(),
                        enozunu::ResolvedOrigin::Local { resolved_path } => {
                            format!("local: {resolved_path}")
                        }
                    };
                    println!(
                        "materialized {} `{}` -> {} ({})",
                        entry.kind.as_str(),
                        entry.source_name,
                        entry.target_rel_path,
                        origin
                    );
                }
                println!(
                    "recorded provenance in {}",
                    enozunu::provenance::PROVENANCE_REL_PATH
                );
            })
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
