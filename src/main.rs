use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use enozunu::diagnostics::Diagnostic;
use enozunu::git::CommandGitResolver;
use enozunu::MANIFEST_FILE_NAME;

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
    /// Parse and validate the manifest without materializing anything.
    Validate {
        /// Path to the manifest. Defaults to enozunu.consumer.kdl in the project root.
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Project root directory. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        project_root: PathBuf,
    },
    /// Resolve sources and materialize them into target AI-native paths.
    Materialize {
        /// Path to the manifest. Defaults to enozunu.consumer.kdl in the project root.
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
        Command::Validate {
            manifest,
            project_root,
        } => {
            let manifest_path = manifest.unwrap_or_else(|| project_root.join(MANIFEST_FILE_NAME));
            enozunu::load_manifest(&manifest_path).map(|_| {
                println!("{} is valid", manifest_path.display());
            })
        }
        Command::Materialize {
            manifest,
            project_root,
        } => {
            let manifest_path = manifest.unwrap_or_else(|| project_root.join(MANIFEST_FILE_NAME));
            let resolver = CommandGitResolver::new(project_root.join(".enozunu/cache"));
            enozunu::run_materialize(&manifest_path, &project_root, &resolver).map(|entries| {
                for entry in &entries {
                    println!(
                        "materialized {} `{}` -> {} ({})",
                        entry.kind.as_str(),
                        entry.source_name,
                        entry.target_rel_path,
                        entry.resolved_revision
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
