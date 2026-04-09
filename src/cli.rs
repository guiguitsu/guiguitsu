use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{self, Config};
use crate::verbose;
use crate::{git_utils, jujutsu, stacks};

pub fn print_help(bin_name: &str) {
    print!(
        "\
Usage: {bin} [-C <path>] [OPTIONS] [<repo-path>]
       {bin} [-C <path>] create-config [<repo-path>] --workspace-branch=<branch> [--workspace-remote=<remote>] [--trunk=<branch>] [-A <sha>]

Options:
  -C <path>                        Change to <path> before doing anything
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
  move -r|-s <sha> <after_commit> [<path>]  Rebase <sha> after <after_commit>, then
                                   move the working copy onto the merge commit
  new <parent> [-m <message>]      Create a new commit after <parent> without
                                   editing it (jj new -A <parent> --no-edit)
  new-stack <branch>               Add <branch> as a parent of the workspace merge commit,
                                   creating it off trunk if it does not exist
  rm-stack [<branch>]              Remove <branch> from the workspace merge commit parents
                                   and from the config. With no arg, list removable stacks.
",
        bin = bin_name
    );
}

pub enum CliCommand {
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
    ListStacks {
        repo_path: PathBuf,
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
    Move {
        repo_path: PathBuf,
        sha: String,
        after_commit: String,
    },
    New {
        repo_path: PathBuf,
        parent: String,
        message: Option<String>,
    },
}

pub fn parse_command(args: Vec<String>) -> Result<CliCommand> {
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

    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
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

    if args.first().map(String::as_str) == Some("rm-stack") {
        let repo_path = std::env::current_dir().expect("failed to get current directory");
        match args.get(1) {
            Some(name) if !name.is_empty() => {
                return Ok(CliCommand::RemoveStack { repo_path, stack_name: name.to_string() });
            }
            _ => return Ok(CliCommand::ListStacks { repo_path }),
        }
    }

    if args.first().map(String::as_str) == Some("new-stack") {
        let branch_name = args.get(1)
            .ok_or_else(|| anyhow::anyhow!("new-stack requires a <branch> argument"))?
            .to_string();
        if branch_name.is_empty() {
            bail!("new-stack branch cannot be empty");
        }
        let repo_path = args.get(2).map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory"));
        return Ok(CliCommand::ApplyBranch { repo_path, branch_name });
    }

    if args.first().map(String::as_str) == Some("move") {
        return parse_move_command(&args[1..]);
    }

    if args.first().map(String::as_str) == Some("new") {
        return parse_new_command(&args[1..]);
    }

    parse_run_command(args)
}

fn parse_new_command(args: &[String]) -> Result<CliCommand> {
    let mut parent: Option<String> = None;
    let mut message: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-m" {
            i += 1;
            let val = args.get(i).ok_or_else(|| anyhow::anyhow!("-m requires a message argument"))?;
            message = Some(val.to_string());
        } else if let Some(val) = arg.strip_prefix("-m") {
            message = Some(val.to_string());
        } else if let Some(val) = arg.strip_prefix("--message=") {
            message = Some(val.to_string());
        } else if !arg.starts_with('-') {
            if parent.is_some() {
                bail!("unexpected argument: {arg}");
            }
            parent = Some(arg.to_string());
        } else {
            bail!("unknown argument: {arg}");
        }
        i += 1;
    }

    let parent = parent.ok_or_else(|| anyhow::anyhow!("new requires a <parent> argument"))?;
    let repo_path = std::env::current_dir().expect("failed to get current directory");

    Ok(CliCommand::New { repo_path, parent, message })
}

fn parse_move_command(args: &[String]) -> Result<CliCommand> {
    let mut sha: Option<String> = None;
    let mut after_commit: Option<String> = None;
    let mut repo_arg: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-r" || arg == "-s" {
            i += 1;
            let val = args.get(i).ok_or_else(|| anyhow::anyhow!("{arg} requires a SHA argument"))?;
            if val.is_empty() {
                bail!("{arg} SHA cannot be empty");
            }
            sha = Some(val.to_string());
        } else if !arg.starts_with('-') {
            if after_commit.is_none() {
                after_commit = Some(arg.to_string());
            } else if repo_arg.replace(PathBuf::from(arg)).is_some() {
                bail!("unexpected argument: {arg}");
            }
        } else {
            bail!("unknown argument: {arg}");
        }
        i += 1;
    }

    let sha = sha.ok_or_else(|| anyhow::anyhow!("move requires -r <sha> or -s <sha>"))?;
    let after_commit = after_commit.ok_or_else(|| anyhow::anyhow!("move requires an <after_commit> argument"))?;
    let repo_path = repo_arg
        .unwrap_or_else(|| std::env::current_dir().expect("failed to get current directory"));

    Ok(CliCommand::Move { repo_path, sha, after_commit })
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

    if let Some(stack_name) = remove_stack {
        return Ok(CliCommand::RemoveStack { repo_path, stack_name });
    }

    if reread_stacks {
        return Ok(CliCommand::RereadStacks { repo_path });
    }

    Ok(CliCommand::Run { repo_path, print_merge_command, verbose })
}

/// Dispatch every command except `Run`. Returns `Ok(true)` if the command was
/// handled here, or `Ok(false)` if the caller (a GUI binary) needs to handle
/// `CliCommand::Run` itself.
pub fn dispatch_non_gui(cmd: CliCommand) -> Result<bool> {
    match cmd {
        CliCommand::Help => {
            // Default help text uses generic name; binaries that want a custom
            // name should call print_help() themselves before dispatching.
            print_help("guiguitsu");
            Ok(true)
        }
        CliCommand::CreateConfig { repo_path, config, after_sha, merge_commit } => {
            create_config(repo_path, config, after_sha, merge_commit)?;
            Ok(true)
        }
        CliCommand::Stacks { repo_path, verbose } => {
            print_stacks(repo_path, verbose)?;
            Ok(true)
        }
        CliCommand::ApplyBranch { repo_path, branch_name } => {
            apply_branch(repo_path, branch_name)?;
            Ok(true)
        }
        CliCommand::RemoveStack { repo_path, stack_name } => {
            remove_stack(repo_path, stack_name)?;
            Ok(true)
        }
        CliCommand::ListStacks { repo_path } => {
            list_removable_stacks(repo_path)?;
            Ok(true)
        }
        CliCommand::RereadStacks { repo_path } => {
            reread_stacks(repo_path)?;
            Ok(true)
        }
        CliCommand::Rebase { repo_path } => {
            rebase_stacks(repo_path)?;
            Ok(true)
        }
        CliCommand::Advance { repo_path, push, dry } => {
            advance_bookmarks(repo_path, push, dry)?;
            Ok(true)
        }
        CliCommand::Move { repo_path, sha, after_commit } => {
            move_commit(repo_path, sha, after_commit)?;
            Ok(true)
        }
        CliCommand::New { repo_path, parent, message } => {
            new_commit(repo_path, parent, message)?;
            Ok(true)
        }
        CliCommand::Run { .. } => Ok(false),
    }
}

pub fn create_config(repo_path: PathBuf, mut config: Config, after_sha: Option<String>, merge_commit: Option<String>) -> Result<()> {
    config.validate(&repo_path)?;

    if let Some(ref mc) = config.merge_commit {
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

pub fn apply_branch(repo_path: PathBuf, branch_name: String) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    let mut config = Config::load(&repo_path)?;

    let (merge_sha, mut parents) = git_utils::find_workspace_merge_commit(&repo_path)?;

    let trunk_remote = format!("{}@{}", config.trunk, config.workspace_remote);
    let new_commit_sha = jujutsu::new_no_edit_on(&repo_path, &trunk_remote)?;
    parents.push(new_commit_sha);

    let merge_change_id = jujutsu::to_change_id(&repo_path, &merge_sha)?;
    jujutsu::rebase_merge_commit(&repo_path, &merge_change_id, &parents, true)?;

    config.stacks.push(config::StackEntry {
        name: branch_name.clone(),
        local_branch: Some(branch_name.clone()),
        remote_branch: Some(format!("{}@{}", branch_name, config.workspace_remote)),
    });
    config.save(&repo_path)?;
    jujutsu::absorb(&repo_path, &[config::FILE_NAME])?;
    Ok(())
}

pub fn list_removable_stacks(repo_path: PathBuf) -> Result<()> {
    let config = Config::load(&repo_path)?;
    let names: Vec<&str> = config.stacks.iter()
        .map(|s| s.name.as_str())
        .filter(|n| *n != "workspace" && *n != config.trunk)
        .collect();
    if names.is_empty() {
        println!("No removable stacks.");
    } else {
        println!("Removable stacks:");
        for n in names {
            println!("  {n}");
        }
    }
    Ok(())
}

pub fn remove_stack(repo_path: PathBuf, stack_name: String) -> Result<()> {
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

    let merge_change_id = jujutsu::to_change_id(&repo_path, &merge_sha)?;
    jujutsu::rebase_merge_commit(&repo_path, &merge_change_id, &new_parents, true)?;

    config.stacks.remove(stack_index);
    config.save(&repo_path)?;
    jujutsu::absorb(&repo_path, &[config::FILE_NAME])?;

    Ok(())
}

pub fn reread_stacks(repo_path: PathBuf) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    let mut config = Config::load(&repo_path)?;
    let (_merge_sha, git_parents) = git_utils::find_workspace_merge_commit(&repo_path)?;

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

pub fn rebase_stacks(repo_path: PathBuf) -> Result<()> {
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

    let merge_sha = jujutsu::to_sha1(repo_path, &merge_ref)?;
    let mut parents = git_utils::parent_shas(repo_path, &merge_sha)?;

    let trunk_index = config.stacks.iter().position(|s| s.name == config.trunk)
        .ok_or_else(|| anyhow::anyhow!("trunk '{}' not found in config stacks", config.trunk))?;

    if trunk_index >= parents.len() {
        bail!(
            "trunk index {} is out of range (merge commit has {} parents)",
            trunk_index, parents.len()
        );
    }

    let trunk_remote_ref = format!("{}@{}", config.trunk, config.workspace_remote);
    parents[trunk_index] = trunk_remote_ref.clone();

    jujutsu::rebase_source(repo_path, &merge_ref, &parents)?;

    jujutsu::new_only(repo_path, &merge_ref)?;

    println!("Rebased workspace merge commit onto {}", &trunk_remote_ref);

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

pub fn advance_bookmarks(repo_path: PathBuf, push: bool, dry: bool) -> Result<()> {
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
        let on_head = bookmarks_map.get(&head.commit_id)
            .map(|names| names.iter().any(|n| n == local_branch))
            .unwrap_or(false);
        if !on_head {
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

pub fn new_commit(repo_path: PathBuf, parent: String, message: Option<String>) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    jujutsu::new_after(&repo_path, &parent, message.as_deref())?;
    Ok(())
}

pub fn move_commit(repo_path: PathBuf, sha: String, after_commit: String) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    let config = Config::load(&repo_path)?;

    jujutsu::rebase_after(&repo_path, &sha, &after_commit)?;

    if let Some(ref merge_commit) = config.merge_commit {
        jujutsu::new_only(&repo_path, merge_commit)?;
    }

    Ok(())
}

pub fn print_stacks(repo_path: PathBuf, verbose: bool) -> Result<()> {
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
                let empty_suffix = if jujutsu::is_empty_commit(&repo_path, &commit.commit_id).unwrap_or(false) {
                    format!(" {RED}(empty){RESET}")
                } else {
                    String::new()
                };
                println!("  {DIM}{} / {}{RESET} {}{bookmark_suffix}{empty_suffix}", &commit.change_id[..8.min(commit.change_id.len())], &commit.commit_id[..8.min(commit.commit_id.len())], commit.description);
            }
        }
    }
    if let Some(ref merge_ref) = config.merge_commit {
        let unstacked = jujutsu::descendants_of(&repo_path, merge_ref)?;
        if !unstacked.is_empty() {
            println!("{CYAN}Unstacked{RESET}");
            for commit in &unstacked {
                let empty_suffix = if jujutsu::is_empty_commit(&repo_path, &commit.commit_id).unwrap_or(false) {
                    format!(" {RED}(empty){RESET}")
                } else {
                    String::new()
                };
                println!("  {DIM}{} / {}{RESET} {}{empty_suffix}", &commit.change_id[..8.min(commit.change_id.len())], &commit.commit_id[..8.min(commit.commit_id.len())], commit.description);
                if let Ok(files_output) = git_utils::run_command(
                    "git",
                    &["diff-tree", "--no-commit-id", "--name-only", "-r", &commit.commit_id],
                    Some(&repo_path),
                ) {
                    for file in files_output.lines() {
                        let file = file.trim();
                        if !file.is_empty() {
                            println!("    {GREEN}{file}{RESET}");
                        }
                    }
                }
            }
            println!();
            let sha_placeholder = if unstacked.len() == 1 {
                unstacked[0].change_id[..8.min(unstacked[0].change_id.len())].to_string()
            } else {
                "<sha1>".to_string()
            };
            println!("Move commits with: gg move -r {sha_placeholder} <stack_commit>");
        }
    }

    if needs_rebase {
        println!();
        println!("Rebase your stacks with \"gg rebase\"");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::{Context, Result, anyhow};

    use super::{apply_branch, new_commit, remove_stack};
    use crate::git_utils::{find_workspace_merge_commit, parent_shas, run_command};
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
        assert!(names.contains(&"foo"), "expected 'foo' in stacks, got: {names:?}");
        Ok(())
    }

    struct TempNewRepo {
        path: PathBuf,
    }

    impl TempNewRepo {
        fn create() -> Result<Self> {
            let mut path = std::env::temp_dir();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("time went backwards")?
                .as_nanos();
            path.push(format!("guiguitsu-new-test-{now}-{}", std::process::id()));

            let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repos/test_new.sh");
            let output = Command::new("bash")
                .arg(script)
                .arg(&path)
                .output()
                .context("failed to execute test_new.sh")?;

            if !output.status.success() {
                return Err(anyhow!(
                    "test_new.sh failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }

            Ok(Self { path })
        }
    }

    impl Drop for TempNewRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn new_commit_creates_child_after_parent_with_message() -> Result<()> {
        let repo = TempNewRepo::create()?;

        let parent_sha = run_command("git", &["rev-parse", "main"], Some(&repo.path))?;

        new_commit(repo.path.clone(), parent_sha.clone(), Some("my new commit".to_string()))?;

        // Find the commit whose subject is "my new commit" and verify its parent is `parent_sha`.
        let new_sha = run_command(
            "git",
            &["log", "--all", "--format=%H", "--grep=^my new commit$"],
            Some(&repo.path),
        )?;
        let new_sha = new_sha.lines().next().unwrap_or("").to_string();
        assert!(!new_sha.is_empty(), "expected to find commit with subject 'my new commit'");
        assert_eq!(parent_shas(&repo.path, &new_sha)?, vec![parent_sha]);

        // --no-edit: working copy should not be on the new commit.
        let at_sha = run_command("jj", &["log", "-r", "@", "--no-graph", "-T", "commit_id"], Some(&repo.path))?;
        assert_ne!(at_sha.trim(), new_sha, "working copy should not be on the new commit");

        Ok(())
    }

    #[test]
    fn apply_branch_updates_config_stacks() -> Result<()> {
        let repo = TempRepo::create()?;

        apply_branch(repo.path.clone(), "bar".to_string())?;

        let config = crate::config::Config::load(&repo.path)?;
        assert_eq!(
            config.stacks.last().map(|s| s.name.as_str()),
            Some("bar"),
            "expected 'bar' as last stack in config, got: {:?}",
            config.stacks
        );
        Ok(())
    }

    #[test]
    fn remove_stack_drops_branch_from_config_and_merge_parents() -> Result<()> {
        let repo = TempRepo::create()?;

        // Baseline: workspace merge commit has 2 parents (workspace, main).
        let (_merge_sha, baseline_parents) = find_workspace_merge_commit(&repo.path)?;
        assert_eq!(baseline_parents.len(), 2);

        apply_branch(repo.path.clone(), "foo".to_string())?;

        let (_merge_sha, after_add) = find_workspace_merge_commit(&repo.path)?;
        assert_eq!(after_add.len(), 3, "expected 3 parents after new-stack, got: {after_add:?}");

        remove_stack(repo.path.clone(), "foo".to_string())?;

        // Config no longer contains "foo".
        let config = crate::config::Config::load(&repo.path)?;
        assert!(
            config.stacks.iter().all(|s| s.name != "foo"),
            "expected 'foo' removed from config, got: {:?}",
            config.stacks
        );

        // Merge commit is back to 2 parents.
        let (_merge_sha, after_rm) = find_workspace_merge_commit(&repo.path)?;
        assert_eq!(after_rm.len(), 2, "expected 2 parents after rm-stack, got: {after_rm:?}");

        Ok(())
    }
}
