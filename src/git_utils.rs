use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};

use crate::config::{Config, FILE_NAME};

pub struct CommitInfo {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    pub author: String,
    pub timestamp: String,
    pub changed_files: Vec<String>,
}

impl CommitInfo {
    pub fn is_conflicted(&self) -> bool {
        self.changed_files.iter().any(|f| f.contains(".jjconflict"))
    }
}

pub(crate) fn run_command(command: &str, args: &[&str], current_dir: Option<&Path>) -> Result<String> {
    let mut process = Command::new(command);
    if let Some(current_dir) = current_dir {
        process.current_dir(current_dir);
    }

    let rendered_args = if args.is_empty() {
        String::new()
    } else {
        format!(" {}", args.join(" "))
    };
    if let Some(current_dir) = current_dir {
        eprintln!("cd {} && {}{}", current_dir.display(), command, rendered_args);
    } else {
        eprintln!("{}{}", command, rendered_args);
    }

    let output = process
        .args(args)
        .output()
        .with_context(|| format!("failed to run {command} with args: {args:?}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if !stderr.is_empty() { stderr } else { stdout };
        return Err(anyhow!("{command} command failed with args {:?}: {details}", args));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    run_command("git", args, Some(repo_path))
}

fn validate_tool_requirements() -> Result<()> {
    run_command("git", &["--version"], None)?;
    run_command("jj", &["--version"], None)?;
    Ok(())
}

pub fn init_repo(repo_path: &Path, config: &Config) -> Result<()> {
    validate_tool_requirements()?;

    if has_staged_changed(repo_path)? {
        return Err(anyhow!(
            "cannot initialize guiguitsu in {} while staged git changes are present",
            repo_path.display()
        ));
    }

    if !repo_path.join(".jj").is_dir() {
        run_command("jj", &["git", "init", "--colocate"], Some(repo_path))?;
    }

    crate::jujutsu::ensure_user_config(repo_path)?;

    config.validate(repo_path)?;
    config.save(repo_path)?;

    if has_file_changes(repo_path, FILE_NAME)? {
        if local_branch_exists(repo_path, &config.workspace_branch)? {
            bail!("workspace branch '{}' already exists", config.workspace_branch);
        }
        run_git(repo_path, &["checkout", "-b", &config.workspace_branch, &config.trunk])?;
        run_git(repo_path, &["add", FILE_NAME])?;
        run_git(repo_path, &["commit", "-m", "Add guiguitsu configuration"])?;
        let head = current_head_sha(repo_path)?;
        let trunk_sha = run_git(repo_path, &["rev-parse", &config.trunk])?;
        crate::jujutsu::create_merge_commit(
            repo_path,
            "Special workspace merge commit",
            &[&head, &trunk_sha],
            true,
        )?;
    }

    Ok(())
}

fn has_file_changes(repo_path: &Path, file_name: &str) -> Result<bool> {
    let output = run_git(repo_path, &["status", "--porcelain", "--", file_name])?;
    Ok(!output.is_empty())
}

pub(crate) fn local_branch_exists(repo_path: &Path, branch: &str) -> Result<bool> {
    let ref_name = format!("refs/heads/{branch}");
    match run_git(repo_path, &["rev-parse", "--verify", &ref_name]) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

pub fn validate_startup_requirements(repo_path: &Path) -> Result<()> {
    validate_tool_requirements()?;

    if !repo_path.join(".jj").is_dir() {
        return Err(anyhow!(
            "missing .jj in repo. Run: guiguitsu init --workspace-branch=<branch> --workspace-remote=<remote> --trunk=<main>"
        ));
    }

    if !repo_path.join(FILE_NAME).is_file() {
        return Err(anyhow!(
            "missing {} in repo. Run: guiguitsu init --workspace-branch=<branch> --workspace-remote=<remote> --trunk=<main>",
            FILE_NAME
        ));
    }

    Ok(())
}

pub fn has_staged_changed(repo_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("diff-index")
        .arg("--quiet")
        .arg("--cached")
        .arg("HEAD")
        .arg("--")
        .current_dir(repo_path)
        .output()
        .with_context(|| format!("failed to run git diff-index in {}", repo_path.display()))?;

    match output.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let details = if !stderr.is_empty() { stderr } else { stdout };
            Err(anyhow!(
                "git diff-index --quiet --cached HEAD -- failed in {}: {details}",
                repo_path.display()
            ))
        }
    }
}

pub fn ensure_remote_exists(repo_path: &Path, remote: &str) -> Result<()> {
    run_git(repo_path, &["remote", "get-url", remote])
        .with_context(|| format!("remote '{remote}' does not exist in {}", repo_path.display()))?;
    Ok(())
}

pub fn current_head_sha(repo_path: &Path) -> Result<String> {
    run_git(repo_path, &["rev-parse", "HEAD"])
}

/// Returns true if `sha` is an ancestor of `of_ref` (or equal to it).
pub fn is_ancestor(repo_path: &Path, sha: &str, of_ref: &str) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["merge-base", "--is-ancestor", sha, of_ref])
        .output()
        .with_context(|| format!("failed to run git merge-base in {}", repo_path.display()))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(anyhow!("git merge-base --is-ancestor failed: {stderr}"))
        }
    }
}

pub fn resolve_ref(repo_path: &Path, git_ref: &str) -> Result<String> {
    run_git(repo_path, &["rev-parse", git_ref])
}

pub fn create_branch(repo_path: &Path, branch: &str, start_point: &str) -> Result<()> {
    run_git(repo_path, &["branch", branch, start_point])?;
    Ok(())
}

pub fn find_workspace_merge_commit(repo_path: &Path) -> Result<(String, Vec<String>)> {
    let mut sha = current_head_sha(repo_path)?;
    loop {
        let parents = parent_shas(repo_path, &sha)?;
        if parents.len() >= 2 {
            return Ok((sha, parents));
        }
        match parents.into_iter().next() {
            Some(p) => sha = p,
            None => bail!("no merge commit found in workspace history"),
        }
    }
}

pub fn parent_shas(repo_path: &Path, commit_sha: &str) -> Result<Vec<String>> {
    let output = run_git(repo_path, &["rev-list", "--parents", "-n", "1", commit_sha])?;
    let mut parts = output.split_whitespace();
    let _commit = parts
        .next()
        .ok_or_else(|| anyhow!("empty output for commit {commit_sha}"))?;

    Ok(parts.map(ToString::to_string).collect())
}

pub fn branch_name_for(repo_path: &Path, sha: &str) -> Result<String> {
    let output = run_git(
        repo_path,
        &["branch", "--points-at", sha, "--format=%(refname:short)"],
    )?;
    output
        .lines()
        .next()
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("no branch points at {sha}"))
}

pub fn commits_in_range(repo_path: &Path, from_sha: &str, to_sha: &str) -> Result<Vec<CommitInfo>> {
    let range = format!("{from_sha}..{to_sha}");
    // Use a recognizable prefix so that commit headers can be distinguished from
    // the file list that --name-only appends after a blank line.  The \x1e record
    // separator cannot be used here because git emits it *before* the file list,
    // so splitting on it would separate each header from its own files.
    let output = run_git(
        repo_path,
        &[
            "log",
            "--name-only",
            "--format=COMMIT:\x1f%H\x1f%an\x1f%ai\x1f%s",
            &range,
        ],
    )?;

    let mut commits: Vec<CommitInfo> = Vec::new();
    let mut current: Option<CommitInfo> = None;

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("COMMIT:\x1f") {
            if let Some(c) = current.take() {
                commits.push(c);
            }
            let mut fields = rest.splitn(4, '\x1f');
            let commit_id = fields.next().unwrap_or("").trim().to_string();
            let author = fields.next().unwrap_or("").trim().to_string();
            let timestamp = fields.next().unwrap_or("").trim().to_string();
            let description = fields.next().unwrap_or("").trim().to_string();
            current = Some(CommitInfo {
                change_id: commit_id.clone(),
                commit_id,
                description,
                author,
                timestamp,
                changed_files: Vec::new(),
            });
        } else {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if let Some(ref mut c) = current {
                    c.changed_files.push(trimmed.to_string());
                }
            }
        }
    }
    if let Some(c) = current {
        commits.push(c);
    }

    Ok(commits)
}

/// Find the SHA of the commit whose subject line exactly matches `description`.
/// Searches all refs (`git log --all`).
pub fn find_commit_by_description(repo_path: &Path, description: &str) -> Result<String> {
    let output = run_git(repo_path, &["log", "--all", "--format=%H\x1f%s"])?;
    for line in output.lines() {
        let mut parts = line.splitn(2, '\x1f');
        let sha = parts.next().unwrap_or("").trim();
        let msg = parts.next().unwrap_or("").trim();
        if msg == description {
            return Ok(sha.to_string());
        }
    }
    bail!("no commit found with description: {description}")
}

pub struct DiffLine {
    pub content: String,
    /// 0 = context, 1 = addition, 2 = deletion, 3 = hunk header
    pub kind: i32,
    pub hunk_id: i32,
}

pub fn get_commit_diff(repo_path: &Path, commit_hash: &str) -> Result<Vec<DiffLine>> {
    use std::cell::Cell;
    use std::cell::RefCell;

    let repo = git2::Repository::open(repo_path)
        .with_context(|| format!("failed to open repo at {}", repo_path.display()))?;
    let oid = git2::Oid::from_str(commit_hash)
        .with_context(|| format!("invalid commit hash: {commit_hash}"))?;
    let commit = repo
        .find_commit(oid)
        .with_context(|| format!("commit {commit_hash} not found"))?;

    let tree = commit.tree().context("failed to get commit tree")?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(
            commit
                .parent(0)
                .context("failed to get parent")?
                .tree()
                .context("failed to get parent tree")?,
        )
    } else {
        None
    };

    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
        .context("failed to compute diff")?;

    let lines: RefCell<Vec<DiffLine>> = RefCell::new(Vec::new());
    let hunk_id: Cell<i32> = Cell::new(0);

    diff.foreach(
        &mut |_, _| true,
        None,
        Some(&mut |_, hunk| {
            let id = hunk_id.get() + 1;
            hunk_id.set(id);
            let header = String::from_utf8_lossy(hunk.header()).trim_end().to_string();
            lines.borrow_mut().push(DiffLine { content: header, kind: 3, hunk_id: id });
            true
        }),
        Some(&mut |_, _, line| {
            let kind = match line.origin() {
                '+' => 1,
                '-' => 2,
                _ => 0,
            };
            let raw = String::from_utf8_lossy(line.content()).trim_end().to_string();
            let content = format!("{}{}", line.origin(), raw);
            lines.borrow_mut().push(DiffLine { content, kind, hunk_id: hunk_id.get() });
            true
        }),
    )
    .context("failed to iterate diff")?;

    Ok(lines.into_inner())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::{Context, Result, anyhow};

    use crate::config::Config;
    use super::{commits_in_range, create_branch, current_head_sha, find_commit_by_description, has_staged_changed, init_repo, parent_shas, run_command, run_git, validate_startup_requirements};

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
            path.push(format!("guiguitsu-test-repo-{now}-{}", std::process::id()));

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
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn git(repo: &std::path::Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .with_context(|| format!("failed to run git with args: {args:?}"))?;

        if !output.status.success() {
            return Err(anyhow!(
                "git command failed with args {:?}: {}",
                args,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    #[test]
    fn current_head_sha_matches_git_rev_parse_head() -> Result<()> {
        let repo = TempRepo::create()?;
        let expected = git(&repo.path, &["rev-parse", "HEAD"])?;

        let actual = current_head_sha(&repo.path)?;

        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn parent_shas_returns_one_parent_for_non_merge_commit() -> Result<()> {
        let repo = TempRepo::create()?;
        let non_merge = git(&repo.path, &["rev-parse", "HEAD^1"])?;

        let parents = parent_shas(&repo.path, &non_merge)?;

        assert_eq!(parents.len(), 1);
        Ok(())
    }

    #[test]
    fn commits_in_range_matches_git_rev_list() -> Result<()> {
        let repo = TempRepo::create()?;
        let from_sha = git(&repo.path, &["rev-parse", "HEAD^1"])?;
        let to_sha = git(&repo.path, &["rev-parse", "branch1"])?;

        let commits = commits_in_range(&repo.path, &from_sha, &to_sha)?;

        let actual: HashSet<String> = commits.iter().map(|commit| commit.commit_id.clone()).collect();
        let expected_raw = git(&repo.path, &["rev-list", &format!("{from_sha}..{to_sha}")])?;
        let expected: HashSet<String> = expected_raw.lines().map(ToString::to_string).collect();

        assert_eq!(actual, expected);
        assert!(commits.iter().all(|commit| !commit.description.is_empty()));
        assert!(commits.iter().all(|commit| !commit.author.is_empty()));
        assert!(commits.iter().all(|commit| !commit.timestamp.is_empty()));
        assert!(commits.iter().all(|commit| commit.change_id == commit.commit_id));
        Ok(())
    }

    #[test]
    fn has_staged_changed_returns_false_without_staged_changes() -> Result<()> {
        let repo = TempRepo::create()?;

        let has_changes = has_staged_changed(&repo.path)?;

        assert!(!has_changes);
        Ok(())
    }

    #[test]
    fn has_staged_changed_returns_true_with_staged_changes() -> Result<()> {
        let repo = TempRepo::create()?;
        let file_path = repo.path.join("staged.txt");

        fs::write(&file_path, "staged change\n").context("failed to write staged file")?;
        git(&repo.path, &["add", "staged.txt"])?;

        let has_changes = has_staged_changed(&repo.path)?;

        assert!(has_changes);
        Ok(())
    }

    #[test]
    fn init_repo_fails_with_staged_changes() -> Result<()> {
        let repo = TempRepo::create()?;
        let file_path = repo.path.join("staged.txt");

        fs::write(&file_path, "staged change\n").context("failed to write staged file")?;
        git(&repo.path, &["add", "staged.txt"])?;

        let config = Config {
            workspace_branch: "guiguitsu/test".to_string(),
            workspace_remote: "origin".to_string(),
            trunk: "main".to_string(),
        };
        let error = init_repo(&repo.path, &config).expect_err("expected staged-change bailout");

        assert!(error.to_string().contains("while staged git changes are present"));
        Ok(())
    }

    #[test]
    fn is_conflicted_returns_true_after_abandoning_parent_commit() -> Result<()> {
        let repo = TempRepo::create()?;

        let abandon_sha = find_commit_by_description(&repo.path, "add main.cpp with hello world")?;
        run_command(
            "jj",
            &["--no-pager", "--ignore-working-copy", "abandon", &abandon_sha],
            Some(&repo.path),
        )?;

        // After abandon, git history contains both the old and the new (rebased)
        // SHA for "use std::format in main.cpp". Find the one that is conflicted
        // by checking which SHA has .jjconflict files in its diff.
        let all_log = run_git(&repo.path, &["log", "--all", "--format=%H\x1f%s"])?;
        let conflicted_sha = all_log
            .lines()
            .find_map(|line: &str| {
                let mut parts = line.splitn(2, '\x1f');
                let sha = parts.next()?.trim();
                let msg = parts.next()?.trim();
                if msg != "use std::format in main.cpp" {
                    return None;
                }
                let files = run_git(&repo.path, &["diff-tree", "--no-commit-id", "-r", "--name-only", sha]).ok()?;
                if files.lines().any(|f: &str| f.contains(".jjconflict")) {
                    Some(sha.to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("no conflicted SHA found for 'use std::format in main.cpp'"))?;

        let parent_sha = parent_shas(&repo.path, &conflicted_sha)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("conflicted commit has no parent"))?;

        let commits = commits_in_range(&repo.path, &parent_sha, &conflicted_sha)?;
        let commit = commits
            .iter()
            .find(|c| c.description == "use std::format in main.cpp")
            .ok_or_else(|| anyhow!("commit not found in range"))?;

        assert!(commit.is_conflicted(), "expected commit to be conflicted after abandoning its parent");
        Ok(())
    }

    #[test]
    fn validate_startup_requirements_fails_when_jj_metadata_is_missing() -> Result<()> {
        let mut path = std::env::temp_dir();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("time went backwards")?
            .as_nanos();
        path.push(format!("guiguitsu-no-jj-test-{now}-{}", std::process::id()));
        fs::create_dir_all(&path).context("failed to create temp dir")?;
        git(&path, &["init"])?;
        git(&path, &["config", "user.name", "Test"])?;
        git(&path, &["config", "user.email", "test@example.com"])?;

        let result = validate_startup_requirements(&path);
        let _ = fs::remove_dir_all(&path);
        let error = result.expect_err("expected missing .jj bailout");

        assert!(error.to_string().contains("missing .jj in repo"));
        Ok(())
    }

    #[test]
    fn add_branch_creates_local_branch_at_trunk() -> Result<()> {
        let repo = TempRepo::create()?;

        // on_add_branch uses config.trunk ("main") as the start point.
        // Verify the branch is created and points at the same commit as main.
        create_branch(&repo.path, "v1", "main")?;

        let v1_sha = run_git(&repo.path, &["rev-parse", "v1"])?;
        let main_sha = run_git(&repo.path, &["rev-parse", "main"])?;
        assert_eq!(v1_sha, main_sha, "v1 should point at the same commit as main");
        Ok(())
    }
}