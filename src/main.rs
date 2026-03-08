mod config;
mod stacks;
mod models;
mod git_utils;

use std::path::PathBuf;

use anyhow::{Result, bail};
use config::Config;

slint::include_modules!();

fn main() -> Result<()> {
    match parse_command(std::env::args().skip(1).collect())? {
        CliCommand::Init { repo_path, config } => {
            git_utils::ensure_remote_exists(&repo_path, &config.workspace_remote)?;
            git_utils::ensure_remote_branch_exists(&repo_path, &config.workspace_remote, &config.trunk)?;
            config.save(&repo_path)?;
            println!("Wrote {}", Config::path(&repo_path).display());
            Ok(())
        }
        CliCommand::Run { repo_path, print_stacks } => run_app(repo_path, print_stacks),
    }
}

enum CliCommand {
    Init {
        repo_path: PathBuf,
        config: Config,
    },
    Run {
        repo_path: PathBuf,
        print_stacks: bool,
    },
}

fn parse_command(args: Vec<String>) -> Result<CliCommand> {
    if args.first().map(String::as_str) == Some("init") {
        return parse_init_command(&args[1..]);
    }

    parse_run_command(&args)
}

fn parse_init_command(args: &[String]) -> Result<CliCommand> {
    let mut repo_arg: Option<PathBuf> = None;
    let mut workspace_branch: Option<String> = None;
    let mut workspace_remote: Option<String> = None;
    let mut trunk: Option<String> = None;

    for arg in args {
        if let Some(value) = arg.strip_prefix("--workspace-branch=") {
            if value.is_empty() {
                bail!("--workspace-branch cannot be empty");
            }
            workspace_branch = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--workspace-remote=") {
            if value.is_empty() {
                bail!("--workspace-remote cannot be empty");
            }
            workspace_remote = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--trunk=") {
            if value.is_empty() {
                bail!("--trunk cannot be empty");
            }
            trunk = Some(value.to_string());
        } else if arg.starts_with("--") {
            bail!("unknown argument: {arg}");
        } else if repo_arg.replace(PathBuf::from(arg)).is_some() {
            bail!("unexpected argument: {arg}");
        }
    }

    Ok(CliCommand::Init {
        repo_path: repo_arg
            .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory")),
        config: Config {
            workspace_branch: workspace_branch
                .ok_or_else(|| anyhow::anyhow!("missing required argument: --workspace-branch=<branch>"))?,
            workspace_remote: workspace_remote
                .ok_or_else(|| anyhow::anyhow!("missing required argument: --workspace-remote=<remote>"))?,
            trunk: trunk.ok_or_else(|| anyhow::anyhow!("missing required argument: --trunk=<main>"))?,
        },
    })
}

fn parse_run_command(args: &[String]) -> Result<CliCommand> {
    let mut repo_arg: Option<PathBuf> = None;
    let mut print_stacks = false;

    for arg in args {
        match arg.as_str() {
            "--print-stacks" => print_stacks = true,
            _ if arg.starts_with("--") => bail!("unknown argument: {arg}"),
            _ if repo_arg.replace(PathBuf::from(arg)).is_some() => bail!("unexpected argument: {arg}"),
            _ => {}
        }
    }

    Ok(CliCommand::Run {
        repo_path: repo_arg
            .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory")),
        print_stacks,
    })
}

fn run_app(repo_path: PathBuf, print_stacks: bool) -> Result<()> {
    use stacks::{GitStackProvider, StackProvider};

    let config = Config::load(&repo_path)?;
    git_utils::ensure_startup_requirements(&repo_path)?;
    git_utils::ensure_remote_exists(&repo_path, &config.workspace_remote)?;
    git_utils::ensure_remote_branch_exists(&repo_path, &config.workspace_remote, &config.trunk)?;
    let provider = GitStackProvider::new(repo_path.clone());
    let stacks = provider.get_stacks()?;

    if print_stacks {
        for stack in &stacks {
            println!("Stack: {}", stack.name);
            for commit in &stack.commits {
                println!("  {} {}", &commit.commit_id[..8.min(commit.commit_id.len())], commit.description);
            }
        }
        return Ok(());
    }

    let model = models::build_stacks_model(&stacks);
    let app = App::new()?;
    app.set_stacks(model);

    app.on_add_branch({
        let base_ref = config.base_ref();
        let repo_path = repo_path.clone();
        move |branch_name| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["branch", branch_name.as_str(), base_ref.as_str()])
                .status();
            match status {
                Ok(s) if !s.success() => eprintln!("git branch {branch_name} {base_ref} failed"),
                Err(e) => eprintln!("failed to run git: {e}"),
                _ => {}
            }
        }
    });

    let app_weak = app.as_weak();
    let repo_path_for_diff = repo_path.clone();
    app.on_select_commit(move |commit_hash| {
        let app = app_weak.unwrap();
        match git_utils::get_commit_diff(&repo_path_for_diff, &commit_hash) {
            Ok(diff_lines) => {
                app.set_diff_lines(models::build_diff_model(&diff_lines));
            }
            Err(e) => {
                eprintln!("failed to get diff for {commit_hash}: {e}");
            }
        }
    });

    app.run()?;
    Ok(())
}
