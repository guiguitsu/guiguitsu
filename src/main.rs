mod config;
mod stacks;
mod models;
mod git_utils;
mod jujutsu;

use std::path::PathBuf;
use std::rc::Rc;

use anyhow::{Context, Result, bail};
use config::Config;

slint::include_modules!();

fn main() -> Result<()> {
    match parse_command(std::env::args().skip(1).collect())? {
        CliCommand::Help => {
            print_help();
            Ok(())
        }
        CliCommand::Init { repo_path, config } => {
            git_utils::init_repo(&repo_path, &config)?;
            println!("Wrote {}", Config::path(&repo_path).display());
            Ok(())
        }
        CliCommand::Run { repo_path, print_stacks } => run_app(repo_path, print_stacks),
        CliCommand::ApplyBranch { repo_path, branch_name } => apply_branch(repo_path, branch_name),
    }
}

fn print_help() {
    print!(
        "\
Usage: guiguitsu [-C <path>] [OPTIONS] [<repo-path>]
       guiguitsu [-C <path>] init [<repo-path>] --workspace-branch=<branch> --workspace-remote=<remote> --trunk=<branch>

Options:
  -C <path>                        Change to <path> before doing anything
  --apply-branch <branch>          Add <branch> as a parent of the workspace merge commit,
                                   creating it off trunk if it does not exist
  --print-stacks                   Print stacks to stdout and exit (no GUI)
  --help                           Show this help message

Init options:
  --workspace-branch=<branch>      Name of the workspace branch to create
  --workspace-remote=<remote>      Name of the git remote (e.g. origin)
  --trunk=<branch>                 Name of the trunk branch (e.g. main)
"
    );
}

enum CliCommand {
    Help,
    Init {
        repo_path: PathBuf,
        config: Config,
    },
    Run {
        repo_path: PathBuf,
        print_stacks: bool,
    },
    ApplyBranch {
        repo_path: PathBuf,
        branch_name: String,
    },
}

fn parse_command(args: Vec<String>) -> Result<CliCommand> {
    let mut args = args.as_slice();

    if args.first().map(String::as_str) == Some("-C") {
        let path = args.get(1).ok_or_else(|| anyhow::anyhow!("-C requires a path argument"))?;
        let path = std::path::Path::new(path);
        if !path.is_dir() {
            bail!("-C path does not exist or is not a directory: {}", path.display());
        }
        std::env::set_current_dir(path)
            .with_context(|| format!("failed to change directory to {}", path.display()))?;
        args = &args[2..];
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(CliCommand::Help);
    }

    if args.first().map(String::as_str) == Some("init") {
        return parse_init_command(&args[1..]);
    }

    parse_run_command(args)
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
    let mut apply_branch: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--print-stacks" {
            print_stacks = true;
        } else if let Some(val) = arg.strip_prefix("--apply-branch=") {
            if val.is_empty() {
                bail!("--apply-branch cannot be empty");
            }
            apply_branch = Some(val.to_string());
        } else if arg == "--apply-branch" {
            i += 1;
            let val = args.get(i).ok_or_else(|| anyhow::anyhow!("--apply-branch requires a branch name"))?;
            if val.is_empty() {
                bail!("--apply-branch cannot be empty");
            }
            apply_branch = Some(val.to_string());
        } else if arg.starts_with("--") {
            bail!("unknown argument: {arg}");
        } else if repo_arg.replace(PathBuf::from(arg)).is_some() {
            bail!("unexpected argument: {arg}");
        }
        i += 1;
    }

    let repo_path = repo_arg
        .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory"));

    if let Some(branch_name) = apply_branch {
        return Ok(CliCommand::ApplyBranch { repo_path, branch_name });
    }

    Ok(CliCommand::Run { repo_path, print_stacks })
}

fn apply_branch(repo_path: PathBuf, branch_name: String) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    let config = Config::load(&repo_path)?;

    // Find the merge commit before any jj new calls that would move HEAD away from it.
    let (merge_sha, mut parents) = git_utils::find_workspace_merge_commit(&repo_path)?;

    if !git_utils::local_branch_exists(&repo_path, &branch_name)? {
        let sha = jujutsu::new_at(&repo_path, &config.trunk)?;
        jujutsu::create_bookmark(&repo_path, &branch_name, &sha)?;
    }
    let branch_head = git_utils::resolve_ref(&repo_path, &branch_name)?;
    parents.push(branch_head);

    let new_merge_sha = jujutsu::rebase_merge_commit(&repo_path, &merge_sha, &parents)?;
    // After rebase the working copy may have moved; park it back on top of the merge commit.
    jujutsu::new_at(&repo_path, &new_merge_sha)?;
    Ok(())
}

fn run_app(repo_path: PathBuf, print_stacks: bool) -> Result<()> {
    use stacks::{GitStackProvider, StackProvider};

    git_utils::validate_startup_requirements(&repo_path)?;
    let config = Config::load(&repo_path)?;
    let provider = GitStackProvider::new(repo_path.clone(), config.trunk.clone());
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

    let reload: Rc<dyn Fn()> = {
        let app_weak = app.as_weak();
        let repo_path = repo_path.clone();
        let trunk = config.trunk.clone();
        Rc::new(move || {
            let app = match app_weak.upgrade() { Some(a) => a, None => return };
            let provider = GitStackProvider::new(repo_path.clone(), trunk.clone());
            match provider.get_stacks() {
                Ok(stacks) => app.set_stacks(models::build_stacks_model(&stacks)),
                Err(e) => eprintln!("failed to reload stacks: {e}"),
            }
        })
    };

    app.on_abandon_commit({
        let reload = Rc::clone(&reload);
        let repo_path = repo_path.clone();
        move |commit_id| {
            match jujutsu::abandon_commit(&repo_path, &commit_id) {
                Ok(()) => reload(),
                Err(e) => eprintln!("failed to abandon {commit_id}: {e}"),
            }
        }
    });

    app.on_refresh({
        let reload = Rc::clone(&reload);
        move || reload()
    });

    app.on_add_branch({
        let repo_path = repo_path.clone();
        let reload = Rc::clone(&reload);
        move |branch_name| {
            if let Err(e) = apply_branch(repo_path.clone(), branch_name.to_string()) {
                eprintln!("apply_branch failed: {e}");
            } else {
                reload();
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::{Context, Result, anyhow};

    use super::apply_branch;
    use crate::stacks::{GitStackProvider, StackProvider};

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn create() -> Result<Self> {
            let mut path = std::env::temp_dir();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("time went backwards")?
                .as_nanos();
            path.push(format!("guiguitsu-main-test-{now}-{}", std::process::id()));

            let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repos/repo1.sh");
            let output = Command::new("bash")
                .arg(script)
                .arg(&path)
                .output()
                .context("failed to execute repo1.sh")?;

            if !output.status.success() {
                return Err(anyhow!(
                    "repo1.sh failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }

            Ok(Self { path })
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn apply_branch_new_branch_appears_in_stacks() -> Result<()> {
        let repo = TempRepo::create()?;

        apply_branch(repo.path.clone(), "foo".to_string())?;

        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string());
        let stacks = provider.get_stacks()?;
        let names: Vec<&str> = stacks.iter().map(|s| s.name.as_str()).collect();

        assert!(names.contains(&"foo"), "expected 'foo' in stacks, got: {names:?}");
        Ok(())
    }
}
