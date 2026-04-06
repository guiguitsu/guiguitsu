mod config;
mod stacks;
mod models;
mod git_utils;
mod jujutsu;

use std::path::{Path, PathBuf};
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
        CliCommand::Run { repo_path, print_merge_command, verbose } => run_app(repo_path, print_merge_command, verbose),
        CliCommand::Stacks { repo_path, verbose } => print_stacks(repo_path, verbose),
        CliCommand::ApplyBranch { repo_path, branch_name } => apply_branch(repo_path, branch_name),
        CliCommand::RemoveStack { repo_path, stack_name } => remove_stack(repo_path, stack_name),
        CliCommand::RereadStacks { repo_path } => reread_stacks(repo_path),
        CliCommand::Rebase { repo_path } => rebase_stacks(repo_path),
        CliCommand::Advance { repo_path, push, dry } => advance_bookmarks(repo_path, push, dry),
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
  --print-merge-command            Print the jj rebase command for the merge commit and exit
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
  stacks [-v] [<repo-path>]         Print stacks to stdout and exit (no GUI)
  rebase [<repo-path>]             Rebase workspace merge commit so its trunk
                                   parent points to the latest remote trunk
  advance [--push] [--dry] [<path>]  Advance outdated bookmarks to their stack head
                                   --push also pushes to remote if no remote bookmark exists
                                   --dry print what would be done without making changes
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
        print_merge_command: bool,
        verbose: bool,
    },
    Stacks {
        repo_path: PathBuf,
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
    Advance {
        repo_path: PathBuf,
        push: bool,
        dry: bool,
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

    if args.first().map(String::as_str) == Some("advance") {
        let rest = &args[1..];
        let mut push = false;
        let mut dry = false;
        let mut repo_arg: Option<PathBuf> = None;
        for arg in rest {
            if arg == "--push" {
                push = true;
            } else if arg == "--dry" {
                dry = true;
            } else if !arg.starts_with('-') {
                repo_arg = Some(PathBuf::from(arg));
            }
        }
        let repo_path = repo_arg
            .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory"));
        return Ok(CliCommand::Advance { repo_path, push, dry });
    }

    if args.first().map(String::as_str) == Some("stacks") {
        let rest = &args[1..];
        let mut verbose = false;
        let mut repo_arg: Option<PathBuf> = None;
        for arg in rest {
            if arg == "-v" || arg == "--verbose" {
                verbose = true;
            } else if !arg.starts_with('-') {
                repo_arg = Some(PathBuf::from(arg));
            }
        }
        let repo_path = repo_arg
            .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory"));
        return Ok(CliCommand::Stacks { repo_path, verbose });
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
    let remote = workspace_remote.unwrap_or_else(|| "origin".to_string());

    Ok(CliCommand::CreateConfig {
        repo_path: repo_arg
            .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory")),
        config: Config {
            workspace_branch: workspace_branch
                .ok_or_else(|| anyhow::anyhow!("missing required argument: --workspace-branch=<branch>"))?,
            stacks: vec![
                config::StackEntry {
                    name: "workspace".to_string(),
                    local_branch: Some("workspace".to_string()),
                    remote_branch: Some(format!("workspace@{remote}")),
                },
                config::StackEntry {
                    name: trunk.clone(),
                    local_branch: None,
                    remote_branch: Some(format!("{trunk}@{remote}")),
                },
            ],
            trunk,
            workspace_remote: remote,
            merge_commit: merge_commit.clone(),
        },
        after_sha,
        merge_commit,
    })
}

fn parse_run_command(args: &[String]) -> Result<CliCommand> {
    let mut repo_arg: Option<PathBuf> = None;
    let mut print_merge_command = false;
    let mut verbose = false;
    let mut reread_stacks = false;
    let mut apply_branch: Option<String> = None;
    let mut remove_stack: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--print-merge-command" {
            print_merge_command = true;
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

    Ok(CliCommand::Run { repo_path, print_merge_command, verbose })
}

fn create_config(repo_path: PathBuf, mut config: Config, after_sha: Option<String>, merge_commit: Option<String>) -> Result<()> {
    config.validate(&repo_path)?;

    if let Some(ref mc) = config.merge_commit {
        // Resolve to SHA for the git parent_shas lookup, but keep the original
        // ref (possibly a jj change-id) in the config as-is.
        let sha = jujutsu::to_sha1(&repo_path, mc)?;
        let git_parents = git_utils::parent_shas(&repo_path, &sha)?;
        for i in 2..git_parents.len() {
            let stack_name = format!("stack{}", i - 1);
            config.stacks.push(config::StackEntry {
                name: stack_name.clone(),
                local_branch: Some(stack_name.clone()),
                remote_branch: Some(format!("{}@{}", stack_name, config.workspace_remote)),
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
    let stack_name = format!("stack{}", stack_count + 1);
    config.stacks.push(config::StackEntry {
        name: stack_name.clone(),
        local_branch: Some(stack_name.clone()),
        remote_branch: Some(format!("{}@{}", stack_name, config.workspace_remote)),
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
    let remote = &config.workspace_remote;
    let mut stacks = vec![
        config::StackEntry {
            name: "workspace".to_string(),
            local_branch: Some("workspace".to_string()),
            remote_branch: Some(format!("workspace@{remote}")),
        },
        config::StackEntry {
            name: config.trunk.clone(),
            local_branch: None,
            remote_branch: Some(format!("{}@{remote}", config.trunk)),
        },
    ];
    for i in 2..git_parents.len() {
        let stack_name = format!("stack{}", i - 1);
        stacks.push(config::StackEntry {
            name: stack_name.clone(),
            local_branch: Some(stack_name.clone()),
            remote_branch: Some(format!("{stack_name}@{remote}")),
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

    let op_id = jujutsu::current_op_id(&repo_path)?;

    match rebase_stacks_inner(&repo_path) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Rebase failed: {e}");
            eprintln!("You can undo with: jj op restore {op_id}");
            Err(e)
        }
    }
}

fn rebase_stacks_inner(repo_path: &Path) -> Result<()> {
    let config = Config::load(repo_path)?;

    let merge_ref = config.merge_commit.as_ref()
        .ok_or_else(|| anyhow::anyhow!("no merge_commit in config; run with --print-stacks first or set it manually"))?
        .clone();

    // Resolve to SHA for the git parent_shas lookup, but keep the original
    // ref (possibly a jj change-id) so we pass it to `jj rebase -s` as-is.
    let merge_sha = jujutsu::to_sha1(repo_path, &merge_ref)?;
    let mut parents = git_utils::parent_shas(repo_path, &merge_sha)?;

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

    jujutsu::rebase_source(repo_path, &merge_ref, &parents)?;

    jujutsu::new_only(repo_path, &merge_ref)?;

    println!("Rebased workspace merge commit onto {}", &trunk_remote_ref);

    // Rebase each non-trunk stack's root onto trunk@remote so they pick up
    // the latest trunk.
    {
        use stacks::StackProvider;
        let config_stacks: Vec<String> = config.stacks.iter().map(|s| s.name.clone()).collect();
        let provider = stacks::GitStackProvider::new(repo_path.to_path_buf(), config.trunk.clone(), config_stacks, config.merge_commit.clone());
        let stack_infos = provider.get_stacks()?;
        for si in &stack_infos {
            if si.name == config.trunk {
                continue;
            }
            if let Some(root) = si.root_commit_id() {
                jujutsu::rebase_source_ignore_immutable(repo_path, root, &[trunk_remote_ref.clone()])?;
                println!("Rebased stack '{}' onto {}", si.name, &trunk_remote_ref);
            }
        }
    }

    let conflicts = jujutsu::resolve_list(repo_path)?;
    if !conflicts.is_empty() {
        println!("\nConflicted files:\n{}", conflicts);
        let status = jujutsu::status(repo_path)?;
        println!("\nStatus:\n{}", status);
    }

    Ok(())
}

fn advance_bookmarks(repo_path: PathBuf, push: bool, dry: bool) -> Result<()> {
    use stacks::{GitStackProvider, StackProvider};

    git_utils::validate_startup_requirements(&repo_path)?;
    let config = Config::load(&repo_path)?;
    let config_stacks: Vec<String> = config.stacks.iter().map(|s| s.name.clone()).collect();
    let provider = GitStackProvider::new(repo_path.clone(), config.trunk.clone(), config_stacks, config.merge_commit.clone());
    let stacks = provider.get_stacks()?;
    let bookmarks_map = jujutsu::bookmarks_by_commit(&repo_path)?;

    let mut did_something = false;
    let prefix = if dry { "[dry-run] " } else { "" };
    for (stack, entry) in stacks.iter().zip(config.stacks.iter()) {
        let local_branch = match entry.local_branch.as_deref() {
            Some(b) => b,
            None => continue,
        };
        let head = match stack.commits.first() {
            Some(c) => c,
            None => continue,
        };
        // Check if local_branch is on the head commit already.
        let on_head = bookmarks_map.get(&head.commit_id)
            .map(|names| names.iter().any(|n| n == local_branch))
            .unwrap_or(false);
        if !on_head {
            // Check if the bookmark exists on any other commit in the stack.
            let exists_elsewhere = stack.commits.iter().skip(1).any(|c| {
                bookmarks_map.get(&c.commit_id)
                    .map(|names| names.iter().any(|n| n == local_branch))
                    .unwrap_or(false)
            });
            if exists_elsewhere {
                if dry {
                    println!("{prefix}jj bookmark set {local_branch} -r {}", &head.commit_id);
                } else {
                    jujutsu::set_bookmark(&repo_path, local_branch, &head.commit_id)?;
                }
                println!("{prefix}Advanced bookmark '{}' to {}", local_branch, &head.change_id[..8.min(head.change_id.len())]);
                did_something = true;
            }
        }

        // Push if requested and no remote bookmark exists yet.
        if push {
            if let Some(ref remote_branch) = entry.remote_branch {
                if let Some((rb_name, remote)) = remote_branch.rsplit_once('@') {
                    let has_remote = bookmarks_map.values().any(|names| {
                        names.iter().any(|n| n == remote_branch)
                    });
                    if !has_remote {
                        if dry {
                            println!("{prefix}git push --force-with-lease {remote} {}:refs/heads/{rb_name}", &head.commit_id);
                        } else {
                            git_utils::git_push(&repo_path, remote, &head.commit_id, rb_name)?;
                        }
                        println!("{prefix}Pushed '{}' to {}", rb_name, remote);
                        did_something = true;
                    }
                }
            }
        }
    }

    if !did_something {
        println!("Nothing to do.");
    }

    Ok(())
}

fn print_stacks(repo_path: PathBuf, verbose: bool) -> Result<()> {
    use stacks::{GitStackProvider, StackProvider};

    const RESET: &str = "\x1b[0m";
    const RED: &str = "\x1b[31m";
    const CYAN: &str = "\x1b[36m";
    const GREEN: &str = "\x1b[32m";
    const YELLOW: &str = "\x1b[33m";
    const MAGENTA: &str = "\x1b[35m";
    const DIM: &str = "\x1b[2m";

    git_utils::validate_startup_requirements(&repo_path)?;
    let config = Config::load(&repo_path)?;
    let config_stacks: Vec<String> = config.stacks.iter().map(|s| s.name.clone()).collect();
    let provider = GitStackProvider::new(repo_path.clone(), config.trunk.clone(), config_stacks, config.merge_commit.clone());
    let stacks = provider.get_stacks()?;

    let trunk_remote_ref = config.base_ref();
    let bookmarks_map = jujutsu::bookmarks_by_commit(&repo_path).unwrap_or_default();
    let mut needs_rebase = false;
    for stack in &stacks {
        let is_trunk = stack.name == config.trunk;
        let head_sha = stack.head_commit_id().unwrap_or(&stack.base_commit_id);
        let head_change_id = jujutsu::to_change_id(&repo_path, head_sha).unwrap_or_default();
        let head_subject = git_utils::commit_subject(&repo_path, head_sha).unwrap_or_default();
        if is_trunk {
            let trunk_behind = git_utils::is_ancestor(&repo_path, head_sha, &trunk_remote_ref).unwrap_or(false)
                && !git_utils::is_ancestor(&repo_path, &trunk_remote_ref, head_sha).unwrap_or(true);
            if trunk_behind {
                needs_rebase = true;
            }
            let trunk_notice = if trunk_behind {
                format!(" {RED}(trunk is behind {}){RESET}", trunk_remote_ref)
            } else {
                String::new()
            };
            print!("{CYAN}{}{RESET}", stack.name);
            println!(" ({GREEN}head: {} - {}{RESET}){trunk_notice}", &head_change_id[..8.min(head_change_id.len())], head_subject);
        } else {
            let base = &stack.base_commit_id;
            let base_change_id = jujutsu::to_change_id(&repo_path, base).unwrap_or_default();
            let base_subject = git_utils::commit_subject(&repo_path, base).unwrap_or_default();
            let stack_needs_rebase = git_utils::is_ancestor(&repo_path, base, &trunk_remote_ref).unwrap_or(false)
                && !git_utils::is_ancestor(&repo_path, &trunk_remote_ref, base).unwrap_or(true);
            if stack_needs_rebase {
                needs_rebase = true;
            }
            let rebase_notice = if stack_needs_rebase {
                format!(" {RED}(needs rebase){RESET}")
            } else {
                String::new()
            };
            print!("{CYAN}{}{RESET}", stack.name);
            print!(" ({GREEN}head: {} - {}{RESET}", &head_change_id[..8.min(head_change_id.len())], head_subject);
            print!(", {YELLOW}base: {} - {}{RESET}", &base_change_id[..8.min(base_change_id.len())], base_subject);
            if verbose {
                if let Some(root) = stack.root_commit_id() {
                    let root_change_id = jujutsu::to_change_id(&repo_path, root).unwrap_or_default();
                    let root_subject = git_utils::commit_subject(&repo_path, root).unwrap_or_default();
                    print!(", {MAGENTA}root: {} - {}{RESET}", &root_change_id[..8.min(root_change_id.len())], root_subject);
                }
            }
            println!("){rebase_notice}");
            let head_commit_id = stack.head_commit_id();
            for commit in &stack.commits {
                let bookmark_suffix = if let Some(names) = bookmarks_map.get(&commit.commit_id) {
                    let is_head = head_commit_id == Some(commit.commit_id.as_str());
                    let colored: Vec<String> = names.iter().map(|n| {
                        if !is_head {
                            format!("{RESET}{RED}{n}{RESET}{DIM}")
                        } else {
                            n.clone()
                        }
                    }).collect();
                    format!(" {DIM}[{}]{RESET}", colored.join(", "))
                } else {
                    String::new()
                };
                println!("  {DIM}{} / {}{RESET} {}{bookmark_suffix}", &commit.change_id[..8.min(commit.change_id.len())], &commit.commit_id[..8.min(commit.commit_id.len())], commit.description);
            }
        }
    }
    // Print unstacked commits (descendants of the merge commit).
    if let Some(ref merge_ref) = config.merge_commit {
        const ORANGE: &str = "\x1b[38;5;208m";
        let unstacked = jujutsu::descendants_of(&repo_path, merge_ref)?;
        if !unstacked.is_empty() {
            println!("{CYAN}Unstacked{RESET}");
            for commit in &unstacked {
                let empty_suffix = if jujutsu::is_empty_commit(&repo_path, &commit.commit_id).unwrap_or(false) {
                    format!(" {ORANGE}(empty){RESET}")
                } else {
                    String::new()
                };
                println!("  {DIM}{} / {}{RESET} {}{empty_suffix}", &commit.change_id[..8.min(commit.change_id.len())], &commit.commit_id[..8.min(commit.commit_id.len())], commit.description);
            }
        }
    }

    if needs_rebase {
        println!();
        println!("Rebase your stacks with \"gg rebase\"");
    }

    Ok(())
}

fn run_app(repo_path: PathBuf, print_merge_command: bool, verbose: bool) -> Result<()> {
    use stacks::{GitStackProvider, StackProvider};

    git_utils::validate_startup_requirements(&repo_path)?;
    let config = Config::load(&repo_path)?;
    let config_stacks: Vec<String> = config.stacks.iter().map(|s| s.name.clone()).collect();
    let merge_commit_ref = config.merge_commit.clone();
    let provider = GitStackProvider::new(repo_path.clone(), config.trunk.clone(), config_stacks.clone(), merge_commit_ref.clone());
    let stacks = provider.get_stacks()?;

    if print_merge_command {
        let merge_ref = config.merge_commit.as_ref()
            .ok_or_else(|| anyhow::anyhow!("no merge_commit in config"))?;
        let merge_sha = jujutsu::to_sha1(&repo_path, merge_ref)?;
        let parents = git_utils::parent_shas(&repo_path, &merge_sha)?;

        let trunk_jj_ref = format!("{}@{}", config.trunk, config.workspace_remote);
        let trunk_index = config.stacks.iter().position(|s| s.name == config.trunk)
            .ok_or_else(|| anyhow::anyhow!("trunk '{}' not found in config stacks", config.trunk))?;

        let mut cmd = format!("jj rebase -s {}", merge_ref);
        for (i, parent_sha) in parents.iter().enumerate() {
            if i == trunk_index {
                cmd.push_str(&format!(" -d {}", trunk_jj_ref));
            } else {
                let change_id = jujutsu::to_change_id(&repo_path, parent_sha)?;
                cmd.push_str(&format!(" -d {}", change_id));
            }
        }
        println!("{cmd}");
        return Ok(());
    }

    let model = models::build_stacks_model(&stacks);
    let app = App::new()?;
    app.set_stacks(model);

    let reload: Rc<dyn Fn()> = {
        let app_weak = app.as_weak();
        let repo_path = repo_path.clone();
        let config_stacks = config_stacks.clone();
        let merge_commit_ref = merge_commit_ref.clone();
        Rc::new(move || {
            let app = match app_weak.upgrade() { Some(a) => a, None => return };
            let provider = GitStackProvider::new(repo_path.clone(), config.trunk.clone(), config_stacks.clone(), merge_commit_ref.clone());
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
        let provider = GitStackProvider::new(repo.path.clone(), config.trunk.clone(), config_stacks, None);
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
