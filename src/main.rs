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

fn verbose() -> bool {
    std::env::var("VERBOSE").as_deref() == Ok("1")
}

fn main() -> Result<()> {
    match parse_command(std::env::args().skip(1).collect())? {
        CliCommand::Help => {
            print_help();
            Ok(())
        }
        CliCommand::CreateConfig { repo_path, config, after_sha, merge_commit } => {
            create_config(repo_path, config, after_sha, merge_commit)
        }
        CliCommand::Run { repo_path, print_stacks, verbose } => run_app(repo_path, print_stacks, verbose),
        CliCommand::ApplyBranch { repo_path, branch_name } => apply_branch(repo_path, branch_name),
        CliCommand::RemoveStack { repo_path, stack_name } => remove_stack(repo_path, stack_name),
        CliCommand::RereadStacks { repo_path } => reread_stacks(repo_path),
        CliCommand::Rebase { repo_path } => rebase_stacks(repo_path),
    }
}

fn print_help() {
    print!(
        "\
Usage: guiguitsu [-C <path>] [OPTIONS] [<repo-path>]
       guiguitsu [-C <path>] create-config [<repo-path>] --workspace-branch=<branch> [--workspace-remote=<remote>] [--trunk=<branch>] [-A <sha>]

Options:
  -C <path>                        Change to <path> before doing anything
  --apply-branch <branch>          Add <branch> as a parent of the workspace merge commit,
                                   creating it off trunk if it does not exist
  --remove-stack <name>            Remove a stack from the workspace merge commit
  --print-stacks                   Print stacks to stdout and exit (no GUI)
  -v, --verbose                    Show extra details (e.g. root commit with --print-stacks)
  --reread-stacks                  Re-read merge commit parents from git and update config
  --help                           Show this help message

create-config options:
  --workspace-branch=<branch>      Name of the workspace branch to create
  --workspace-remote=<remote>      Name of the git remote (default: origin)
  --trunk=<branch>                 Name of the trunk branch (default: main)
  -A <sha>                         After creating the config, rebase @ after <sha>
                                   (requires --merge-commit)
  --merge-commit=<sha>             SHA of the workspace merge commit (written to config)

Subcommands:
  rebase [<repo-path>]             Rebase workspace merge commit so its trunk
                                   parent points to the latest remote trunk
"
    );
}

enum CliCommand {
    Help,
    CreateConfig {
        repo_path: PathBuf,
        config: Config,
        after_sha: Option<String>,
        merge_commit: Option<String>,
    },
    Run {
        repo_path: PathBuf,
        print_stacks: bool,
        verbose: bool,
    },
    ApplyBranch {
        repo_path: PathBuf,
        branch_name: String,
    },
    RemoveStack {
        repo_path: PathBuf,
        stack_name: String,
    },
    RereadStacks {
        repo_path: PathBuf,
    },
    Rebase {
        repo_path: PathBuf,
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

    if args.first().map(String::as_str) == Some("create-config") {
        return parse_create_config_command(&args[1..]);
    }

    if args.first().map(String::as_str) == Some("rebase") {
        let repo_path = args.get(1).map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory"));
        return Ok(CliCommand::Rebase { repo_path });
    }

    parse_run_command(args)
}

fn parse_create_config_command(args: &[String]) -> Result<CliCommand> {
    let mut repo_arg: Option<PathBuf> = None;
    let mut workspace_branch: Option<String> = None;
    let mut workspace_remote: Option<String> = None;
    let mut trunk: Option<String> = None;
    let mut after_sha: Option<String> = None;
    let mut merge_commit: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
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
        } else if let Some(value) = arg.strip_prefix("--merge-commit=") {
            if value.is_empty() {
                bail!("--merge-commit cannot be empty");
            }
            merge_commit = Some(value.to_string());
        } else if arg == "-A" {
            i += 1;
            let val = args.get(i).ok_or_else(|| anyhow::anyhow!("-A requires a SHA argument"))?;
            if val.is_empty() {
                bail!("-A cannot be empty");
            }
            after_sha = Some(val.to_string());
        } else if arg.starts_with("--") {
            bail!("unknown argument: {arg}");
        } else if repo_arg.replace(PathBuf::from(arg)).is_some() {
            bail!("unexpected argument: {arg}");
        }
        i += 1;
    }

    if after_sha.is_some() && merge_commit.is_none() {
        bail!("-A requires --merge-commit to be specified");
    }

    let trunk = trunk.unwrap_or_else(|| "main".to_string());

    Ok(CliCommand::CreateConfig {
        repo_path: repo_arg
            .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory")),
        config: Config {
            workspace_branch: workspace_branch
                .ok_or_else(|| anyhow::anyhow!("missing required argument: --workspace-branch=<branch>"))?,
            workspace_remote: workspace_remote.unwrap_or_else(|| "origin".to_string()),
            stacks: vec![
                config::StackEntry { name: "workspace".to_string() },
                config::StackEntry { name: trunk.clone() },
            ],
            trunk,
            merge_commit: merge_commit.clone(),
        },
        after_sha,
        merge_commit,
    })
}

fn parse_run_command(args: &[String]) -> Result<CliCommand> {
    let mut repo_arg: Option<PathBuf> = None;
    let mut print_stacks = false;
    let mut verbose = false;
    let mut reread_stacks = false;
    let mut apply_branch: Option<String> = None;
    let mut remove_stack: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--print-stacks" {
            print_stacks = true;
        } else if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if arg == "--reread-stacks" {
            reread_stacks = true;
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
        } else if let Some(val) = arg.strip_prefix("--remove-stack=") {
            if val.is_empty() {
                bail!("--remove-stack cannot be empty");
            }
            remove_stack = Some(val.to_string());
        } else if arg == "--remove-stack" {
            i += 1;
            let val = args.get(i).ok_or_else(|| anyhow::anyhow!("--remove-stack requires a stack name"))?;
            if val.is_empty() {
                bail!("--remove-stack cannot be empty");
            }
            remove_stack = Some(val.to_string());
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

    if let Some(stack_name) = remove_stack {
        return Ok(CliCommand::RemoveStack { repo_path, stack_name });
    }

    if reread_stacks {
        return Ok(CliCommand::RereadStacks { repo_path });
    }

    Ok(CliCommand::Run { repo_path, print_stacks, verbose })
}

fn create_config(repo_path: PathBuf, mut config: Config, after_sha: Option<String>, merge_commit: Option<String>) -> Result<()> {
    config.validate(&repo_path)?;

    if let Some(ref mc) = config.merge_commit {
        // Resolve to SHA for the git parent_shas lookup, but keep the original
        // ref (possibly a jj change-id) in the config as-is.
        let sha = jujutsu::to_sha1(&repo_path, mc)?;
        let git_parents = git_utils::parent_shas(&repo_path, &sha)?;
        for i in 2..git_parents.len() {
            config.stacks.push(config::StackEntry {
                name: format!("stack{}", i - 1),
            });
        }
    }

    let already_exists = Config::path(&repo_path).is_file();
    config.save(&repo_path)?;

    if verbose() {
        let path = Config::path(&repo_path);
        println!("Wrote {}", path.display());
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        println!("{contents}");
    }

    if already_exists {
        jujutsu::absorb(&repo_path, &[config::FILE_NAME])?;
    } else {
        jujutsu::describe_current(&repo_path, "Add guiguitsu configuration")?;

        if let Some(sha) = after_sha {
            jujutsu::rebase_after(&repo_path, "@", &sha)?;
        }
    }

    if let Some(ref mc) = merge_commit {
        jujutsu::new_at(&repo_path, mc)?;
    }

    Ok(())
}

fn apply_branch(repo_path: PathBuf, branch_name: String) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    let mut config = Config::load(&repo_path)?;

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

    let stack_count = config.stacks.iter().filter(|s| s.name != "workspace" && s.name != config.trunk).count();
    config.stacks.push(config::StackEntry {
        name: format!("stack{}", stack_count + 1),
    });
    config.save(&repo_path)?;
    Ok(())
}

fn remove_stack(repo_path: PathBuf, stack_name: String) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    let mut config = Config::load(&repo_path)?;

    let stack_index = config.stacks.iter().position(|s| s.name == stack_name)
        .ok_or_else(|| anyhow::anyhow!("stack '{}' not found in config", stack_name))?;

    if stack_name == "workspace" || stack_name == config.trunk {
        bail!("cannot remove the '{}' stack", stack_name);
    }

    let (merge_sha, parents) = git_utils::find_workspace_merge_commit(&repo_path)?;

    if stack_index >= parents.len() {
        bail!(
            "stack index {} is out of range (merge commit has {} parents)",
            stack_index, parents.len()
        );
    }

    let new_parents: Vec<String> = parents.into_iter()
        .enumerate()
        .filter(|(i, _)| *i != stack_index)
        .map(|(_, p)| p)
        .collect();

    let new_merge_sha = jujutsu::rebase_merge_commit(&repo_path, &merge_sha, &new_parents)?;
    jujutsu::new_at(&repo_path, &new_merge_sha)?;

    config.stacks.remove(stack_index);
    config.save(&repo_path)?;
    jujutsu::absorb(&repo_path, &[config::FILE_NAME])?;

    Ok(())
}

fn reread_stacks(repo_path: PathBuf) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    let mut config = Config::load(&repo_path)?;
    let (_merge_sha, git_parents) = git_utils::find_workspace_merge_commit(&repo_path)?;

    // git_parents[0] = workspace, [1] = trunk, [2..] = stacks
    let mut stacks = vec![
        config::StackEntry { name: "workspace".to_string() },
        config::StackEntry { name: config.trunk.clone() },
    ];
    for i in 2..git_parents.len() {
        stacks.push(config::StackEntry {
            name: format!("stack{}", i - 1),
        });
    }

    config.stacks = stacks;
    config.save(&repo_path)?;
    jujutsu::absorb(&repo_path, &[config::FILE_NAME])?;

    if verbose() {
        println!("Updated stacks:");
        for (i, entry) in config.stacks.iter().enumerate() {
            println!("  [{}] {}", i, entry.name);
        }
    }
    Ok(())
}

fn rebase_stacks(repo_path: PathBuf) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    let mut config = Config::load(&repo_path)?;

    let merge_ref = config.merge_commit.as_ref()
        .ok_or_else(|| anyhow::anyhow!("no merge_commit in config; run with --print-stacks first or set it manually"))?
        .clone();

    // Resolve to SHA for the git parent_shas lookup, but keep the original
    // ref (possibly a jj change-id) so we pass it to `jj rebase -s` as-is.
    let merge_sha = jujutsu::to_sha1(&repo_path, &merge_ref)?;
    let mut parents = git_utils::parent_shas(&repo_path, &merge_sha)?;

    // The trunk parent is at the index of the trunk entry in config.stacks.
    let trunk_index = config.stacks.iter().position(|s| s.name == config.trunk)
        .ok_or_else(|| anyhow::anyhow!("trunk '{}' not found in config stacks", config.trunk))?;

    if trunk_index >= parents.len() {
        bail!(
            "trunk index {} is out of range (merge commit has {} parents)",
            trunk_index, parents.len()
        );
    }

    // Use jj's trunk@remote syntax so jj resolves it directly.
    let trunk_remote_ref = format!("{}@{}", config.trunk, config.workspace_remote);
    parents[trunk_index] = trunk_remote_ref.clone();

    jujutsu::rebase_source(&repo_path, &merge_ref, &parents)?;

    jujutsu::new_only(&repo_path, &merge_ref)?;

    println!("Rebased workspace merge commit onto {}", &trunk_remote_ref);

    let conflicts = jujutsu::resolve_list(&repo_path)?;
    if !conflicts.is_empty() {
        println!("\nConflicted files:\n{}", conflicts);
        let status = jujutsu::status(&repo_path)?;
        println!("\nStatus:\n{}", status);
    }

    Ok(())
}

fn run_app(repo_path: PathBuf, print_stacks: bool, verbose: bool) -> Result<()> {
    use stacks::{GitStackProvider, StackProvider};

    git_utils::validate_startup_requirements(&repo_path)?;
    let config = Config::load(&repo_path)?;
    let config_stacks: Vec<String> = config.stacks.iter().map(|s| s.name.clone()).collect();
    let provider = GitStackProvider::new(repo_path.clone(), config.trunk.clone(), config_stacks.clone());
    let stacks = provider.get_stacks()?;

    if print_stacks {
        const RESET: &str = "\x1b[0m";
        const RED: &str = "\x1b[31m";
        const CYAN: &str = "\x1b[36m";
        const GREEN: &str = "\x1b[32m";
        const YELLOW: &str = "\x1b[33m";
        const MAGENTA: &str = "\x1b[35m";

        let trunk_remote_ref = config.base_ref();
        for stack in &stacks {
            let is_trunk = stack.name == config.trunk;
            let head_sha = stack.head_commit_id().unwrap_or(&stack.base_commit_id);
            let head_subject = git_utils::commit_subject(&repo_path, head_sha).unwrap_or_default();
            if is_trunk {
                print!("{CYAN}Stack: {}{RESET}", stack.name);
                println!(" ({GREEN}head: {} - {}{RESET})", &head_sha[..8.min(head_sha.len())], head_subject);
            } else {
                let base = &stack.base_commit_id;
                let base_subject = git_utils::commit_subject(&repo_path, base).unwrap_or_default();
                let rebase_notice = if git_utils::is_ancestor(&repo_path, base, &trunk_remote_ref).unwrap_or(false) {
                    format!(" {RED}(needs rebase){RESET}")
                } else {
                    String::new()
                };
                print!("{CYAN}Stack: {}{RESET}", stack.name);
                print!(" ({GREEN}head: {} - {}{RESET}", &head_sha[..8.min(head_sha.len())], head_subject);
                print!(", {YELLOW}base: {} - {}{RESET}", &base[..8.min(base.len())], base_subject);
                if verbose {
                    if let Some(root) = stack.root_commit_id() {
                        let root_subject = git_utils::commit_subject(&repo_path, root).unwrap_or_default();
                        print!(", {MAGENTA}root: {} - {}{RESET}", &root[..8.min(root.len())], root_subject);
                    }
                }
                println!("){rebase_notice}");
                for commit in &stack.commits {
                    println!("  {} {}", &commit.commit_id[..8.min(commit.commit_id.len())], commit.description);
                }
            }
        }
        println!();
        println!("Rebase your stacks with \"gg rebase\"");
        return Ok(());
    }

    let model = models::build_stacks_model(&stacks);
    let app = App::new()?;
    app.set_stacks(model);

    let reload: Rc<dyn Fn()> = {
        let app_weak = app.as_weak();
        let repo_path = repo_path.clone();
        let config_stacks = config_stacks.clone();
        Rc::new(move || {
            let app = match app_weak.upgrade() { Some(a) => a, None => return };
            let provider = GitStackProvider::new(repo_path.clone(), config.trunk.clone(), config_stacks.clone());
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

    app.on_drop_onto_commit(|dragged_id, target_id| {
        println!("[drag] drop onto: {} -> {}", dragged_id, target_id);
    });

    app.on_drop_between_commits(|dragged_id, before_id| {
        if before_id.is_empty() {
            println!("[drag] drop between: {} -> top of stack", dragged_id);
        } else {
            println!("[drag] drop between: {} -> after {}", dragged_id, before_id);
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

        let config = crate::config::Config::load(&repo.path)?;
        let config_stacks: Vec<String> = config.stacks.iter().map(|s| s.name.clone()).collect();
        let provider = GitStackProvider::new(repo.path.clone(), config.trunk.clone(), config_stacks);
        let stacks = provider.get_stacks()?;
        let names: Vec<&str> = stacks.iter().map(|s| s.name.as_str()).collect();

        assert_eq!(names.len(), 3, "expected 3 stacks, got: {names:?}");
        assert!(names.contains(&"stack1"), "expected 'stack1' in stacks, got: {names:?}");
        Ok(())
    }

    #[test]
    fn apply_branch_updates_config_stacks() -> Result<()> {
        let repo = TempRepo::create()?;

        apply_branch(repo.path.clone(), "bar".to_string())?;

        let config = crate::config::Config::load(&repo.path)?;
        assert_eq!(
            config.stacks.last().map(|s| s.name.as_str()),
            Some("stack1"),
            "expected 'stack1' as last stack in config, got: {:?}",
            config.stacks
        );
        Ok(())
    }
}
