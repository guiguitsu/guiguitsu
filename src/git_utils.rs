use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};

use crate::config::FILE_NAME;

#[derive(Debug)]
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
    if std::env::var("VERBOSE").as_deref() == Ok("1") {
        if let Some(current_dir) = current_dir {
            eprintln!("cd {} && {}{}", current_dir.display(), command, rendered_args);
        } else {
            eprintln!("{}{}", command, rendered_args);
        }
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

pub(crate) fn run_command_interactive(command: &str, args: &[&str], current_dir: Option<&Path>) -> Result<()> {
    let mut process = Command::new(command);
    if let Some(current_dir) = current_dir {
        process.current_dir(current_dir);
    }

    let rendered_args = if args.is_empty() {
        String::new()
    } else {
        format!(" {}", args.join(" "))
    };
    if std::env::var("VERBOSE").as_deref() == Ok("1") {
        if let Some(current_dir) = current_dir {
            eprintln!("cd {} && {}{}", current_dir.display(), command, rendered_args);
        } else {
            eprintln!("{}{}", command, rendered_args);
        }
    }

    let status = process
        .args(args)
        .status()
        .with_context(|| format!("failed to run {command} with args: {args:?}"))?;

    if !status.success() {
        bail!("{command} command failed with args {args:?} (exit code: {:?})", status.code());
    }

    Ok(())
}

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    run_command("git", args, Some(repo_path))
}

fn validate_tool_requirements() -> Result<()> {
    run_command("git", &["--version"], None)?;
    run_command("jj", &["--version"], None)?;
    Ok(())
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

/// Checks that the remote tracking ref `refs/remotes/<remote>/<branch>` resolves to a commit.
pub fn validate_remote_branch_exists(repo_path: &Path, remote: &str, branch: &str) -> Result<()> {
    let git_ref = format!("refs/remotes/{remote}/{branch}");
    resolve_ref(repo_path, &git_ref)
        .with_context(|| format!("remote branch '{remote}/{branch}' does not exist"))?;
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

pub fn commit_subject(repo_path: &Path, sha: &str) -> Result<String> {
    run_git(repo_path, &["log", "-1", "--format=%s", sha])
}

pub fn merge_base(repo_path: &Path, a: &str, b: &str) -> Result<String> {
    run_git(repo_path, &["merge-base", a, b])
}

pub fn git_push(repo_path: &Path, remote: &str, sha: &str, branch_name: &str) -> Result<()> {
    let refspec = format!("{sha}:refs/heads/{branch_name}");
    run_git(repo_path, &["push", "--force-with-lease", remote, &refspec])?;
    Ok(())
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

/// Returns true if the commit has more than one parent.
pub fn is_merge_commit(repo_path: &Path, commit_sha: &str) -> Result<bool> {
    let parents = parent_shas(repo_path, commit_sha)?;
    Ok(parents.len() >= 2)
}

/// Returns the SHAs of all commits that have `commit_sha` as a parent,
/// within the given range of refs (e.g. `"--all"` or `"branch_a..branch_b"`).
/// If `range` is empty, searches all refs.
pub fn children_of(repo_path: &Path, commit_sha: &str, range: &[&str]) -> Result<Vec<String>> {
    let mut args = vec!["rev-list", "--parents"];
    if range.is_empty() {
        // Exclude jj internal refs to avoid seeing abandoned operation snapshots.
        args.extend_from_slice(&["--exclude=refs/jj/*", "--all"]);
    } else {
        args.extend_from_slice(range);
    }
    let output = run_git(repo_path, &args)?;

    let mut children = Vec::new();
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let child_sha = match parts.next() {
            Some(s) => s,
            None => continue,
        };
        // Remaining tokens are the parent SHAs of this child
        if parts.any(|p| p == commit_sha) {
            children.push(child_sha.to_string());
        }
    }
    Ok(children)
}

/// Walks forward from `commit_sha` through its children until a merge commit
/// is found. Bails if any commit along the path has more than one child
/// (ambiguous path). Returns the SHA of the first merge commit encountered.
pub fn child_merge_commit(repo_path: &Path, commit_sha: &str) -> Result<String> {
    let mut current = commit_sha.to_string();
    loop {
        let kids = children_of(repo_path, &current, &[])?;
        eprintln!("[child_merge_commit] current={current}, kids={kids:?}");
        match kids.len() {
            0 => bail!("reached tip without finding a merge commit (at {current})"),
            1 => {
                let child = &kids[0];
                if is_merge_commit(repo_path, child)? {
                    eprintln!("[child_merge_commit] found merge commit: {child}");
                    return Ok(child.clone());
                }
                eprintln!("[child_merge_commit] {child} is not a merge, continuing walk");
                current = child.clone();
            }
            _ => bail!(
                "commit {current} has {} children; path is ambiguous",
                kids.len()
            ),
        }
    }
}

/// Checks whether a valid workspace merge commit already exists.
///
/// Walks forward from the workspace branch through `child_merge_commit` and
/// validates that the found merge's parents include both the workspace branch
/// SHA and an ancestor of trunk.
///
/// Returns `Ok(Some(merge_sha))` if a valid merge exists, `Ok(None)` otherwise.
pub fn find_existing_workspace_merge(
    repo_path: &Path,
    workspace_branch: &str,
    trunk: &str,
) -> Result<Option<String>> {
    let workspace_sha = resolve_ref(repo_path, workspace_branch)?;
    eprintln!("[find_existing_workspace_merge] workspace_sha={workspace_sha}");

    let merge_sha = match child_merge_commit(repo_path, &workspace_sha) {
        Ok(sha) => {
            eprintln!("[find_existing_workspace_merge] child_merge_commit => Ok({sha})");
            sha
        }
        Err(e) => {
            eprintln!("[find_existing_workspace_merge] child_merge_commit => Err({e})");
            return Ok(None);
        }
    };

    let parents = parent_shas(repo_path, &merge_sha)?;
    eprintln!("[find_existing_workspace_merge] merge parents={parents:?}");

    // One parent must be the workspace branch SHA
    let contains_workspace = parents.contains(&workspace_sha);
    eprintln!("[find_existing_workspace_merge] parents.contains(workspace_sha)={contains_workspace}");
    if !contains_workspace {
        return Ok(None);
    }

    // Another parent must be an ancestor of trunk (or equal to it)
    let trunk_sha = resolve_ref(repo_path, trunk)?;
    eprintln!("[find_existing_workspace_merge] trunk_sha={trunk_sha}");
    let has_trunk_ancestor = parents.iter().any(|p| {
        let result = p != &workspace_sha && is_ancestor(repo_path, p, &trunk_sha).unwrap_or(false);
        eprintln!("[find_existing_workspace_merge]   parent {p}: p!=workspace={}, is_ancestor={:?}, result={result}",
            p != &workspace_sha,
            is_ancestor(repo_path, p, &trunk_sha));
        result
    });
    eprintln!("[find_existing_workspace_merge] has_trunk_ancestor={has_trunk_ancestor}");

    if has_trunk_ancestor {
        Ok(Some(merge_sha))
    } else {
        Ok(None)
    }
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
    use super::{child_merge_commit, children_of, commits_in_range, create_branch, current_head_sha, find_commit_by_description, find_existing_workspace_merge, has_staged_changed, is_merge_commit, parent_shas, run_command, run_git, validate_startup_requirements};

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn create() -> Result<Self> {
            Self::create_from("repo1.sh")
        }

        fn create_from(script_name: &str) -> Result<Self> {
            let mut path = std::env::temp_dir();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("time went backwards")?
                .as_nanos();
            path.push(format!("guiguitsu-test-repo-{now}-{}", std::process::id()));

            let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/repos")
                .join(script_name);
            let output = Command::new("bash")
                .arg(&script)
                .arg(&path)
                .output()
                .with_context(|| format!("failed to execute {script_name}"))?;

            if !output.status.success() {
                return Err(anyhow!(
                    "{} failed: {}",
                    script_name,
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

    #[test]
    fn is_merge_commit_returns_true_for_merge() -> Result<()> {
        let repo = TempRepo::create()?;
        let head = current_head_sha(&repo.path)?;
        // repo1.sh ends on a workspace merge commit
        assert!(is_merge_commit(&repo.path, &head)?, "HEAD should be a merge commit");
        Ok(())
    }

    #[test]
    fn is_merge_commit_returns_false_for_regular_commit() -> Result<()> {
        let repo = TempRepo::create()?;
        // First parent of the merge is a regular commit
        let parents = parent_shas(&repo.path, &current_head_sha(&repo.path)?)?;
        let first_parent = &parents[0];
        assert!(!is_merge_commit(&repo.path, first_parent)?, "first parent should not be a merge commit");
        Ok(())
    }

    #[test]
    fn children_of_returns_correct_children() -> Result<()> {
        let repo = TempRepo::create_from("test_gitutils.sh")?;

        // Find the root commit (tagged "root" by the script)
        let root_sha = run_git(&repo.path, &["rev-parse", "root"])?;

        // The root commit should have children on both main and feature branches
        let children = children_of(&repo.path, &root_sha, &[])?;
        assert!(children.len() >= 2, "root commit should have at least 2 children, got {}", children.len());
        Ok(())
    }

    #[test]
    fn children_of_merge_commit_has_no_children_at_tip() -> Result<()> {
        let repo = TempRepo::create_from("test_gitutils.sh")?;

        // The merge commit is at the tip of main, so it has no children
        let merge_sha = run_git(&repo.path, &["rev-parse", "merge-commit"])?;
        let children = children_of(&repo.path, &merge_sha, &[])?;
        assert!(children.is_empty(), "merge commit at tip should have no children");
        Ok(())
    }

    #[test]
    fn is_merge_commit_on_test_gitutils_repo() -> Result<()> {
        let repo = TempRepo::create_from("test_gitutils.sh")?;

        let merge_sha = run_git(&repo.path, &["rev-parse", "merge-commit"])?;
        assert!(is_merge_commit(&repo.path, &merge_sha)?, "tagged merge-commit should be a merge");

        let regular_sha = run_git(&repo.path, &["rev-parse", "root"])?;
        assert!(!is_merge_commit(&repo.path, &regular_sha)?, "root commit should not be a merge");
        Ok(())
    }

    #[test]
    fn child_merge_commit_finds_merge_via_linear_path() -> Result<()> {
        let repo = TempRepo::create_from("test_gitutils.sh")?;

        // main3 -> merge-commit is a single-child linear path ending at a merge
        let main3_sha = run_git(&repo.path, &["rev-parse", "main3"])?;
        let expected_merge = run_git(&repo.path, &["rev-parse", "merge-commit"])?;

        let found = child_merge_commit(&repo.path, &main3_sha)?;
        assert_eq!(found, expected_merge);
        Ok(())
    }

    #[test]
    fn child_merge_commit_bails_on_ambiguous_children() -> Result<()> {
        let repo = TempRepo::create_from("test_gitutils.sh")?;

        // root has 3 children (main2, feature1, feature2-1) — ambiguous
        let root_sha = run_git(&repo.path, &["rev-parse", "root"])?;

        let err = child_merge_commit(&repo.path, &root_sha)
            .expect_err("should bail on ambiguous children");
        assert!(err.to_string().contains("ambiguous"), "error: {err}");
        Ok(())
    }

    #[test]
    fn child_merge_commit_bails_at_tip_with_no_merge() -> Result<()> {
        let repo = TempRepo::create_from("test_gitutils.sh")?;

        // feature2 commit 1 is a tip with no children
        let tip_sha = run_git(&repo.path, &["rev-parse", "feature2"])?;

        let err = child_merge_commit(&repo.path, &tip_sha)
            .expect_err("should bail at tip");
        assert!(err.to_string().contains("reached tip"), "error: {err}");
        Ok(())
    }

    #[test]
    fn find_existing_workspace_merge_returns_some_when_merge_exists() -> Result<()> {
        let repo = TempRepo::create_from("repo_init_with_merge.sh")?;

        let result = find_existing_workspace_merge(&repo.path, "workspace", "main")?;
        assert!(result.is_some(), "should find existing workspace merge commit");

        // The returned SHA should be a merge commit
        let merge_sha = result.unwrap();
        assert!(is_merge_commit(&repo.path, &merge_sha)?, "returned commit should be a merge");

        Ok(())
    }

    #[test]
    fn find_existing_workspace_merge_returns_none_without_merge() -> Result<()> {
        let repo = TempRepo::create_from("repo_init.sh")?;

        let result = find_existing_workspace_merge(&repo.path, "workspace", "main")?;
        assert!(result.is_none(), "should not find merge when none exists");

        Ok(())
    }

}