use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

pub struct CommitInfo {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    pub author: String,
    pub timestamp: String,
}

fn run_command(command: &str, args: &[&str], current_dir: Option<&Path>) -> Result<String> {
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

pub fn ensure_startup_requirements(repo_path: &Path) -> Result<()> {
    run_command("git", &["--version"], None)?;
    run_command("jj", &["--version"], None)?;

    if !repo_path.join(".jj").is_dir() {
        run_command("jj", &["git", "init", "--colocate"], Some(repo_path))?;
    }

    Ok(())
}

pub fn ensure_remote_exists(repo_path: &Path, remote: &str) -> Result<()> {
    run_git(repo_path, &["remote", "get-url", remote])
        .with_context(|| format!("remote '{remote}' does not exist in {}", repo_path.display()))?;
    Ok(())
}

pub fn ensure_remote_branch_exists(repo_path: &Path, remote: &str, branch: &str) -> Result<()> {
    run_git(repo_path, &["ls-remote", "--exit-code", "--heads", remote, branch]).with_context(|| {
        format!(
            "branch '{branch}' does not exist on remote '{remote}' in {}",
            repo_path.display()
        )
    })?;
    Ok(())
}

pub fn current_head_sha(repo_path: &Path) -> Result<String> {
    run_git(repo_path, &["rev-parse", "HEAD"])
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
    let output = run_git(
        repo_path,
        &[
            "log",
            "--format=%H%x1f%an%x1f%ai%x1f%s%x1e",
            &range,
        ],
    )?;

    let commits = output
        .split('\x1e')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }

            let mut fields = entry.split('\x1f');
            let commit_id = fields.next()?.trim().to_string();
            let author = fields.next()?.trim().to_string();
            let timestamp = fields.next()?.trim().to_string();
            let description = fields.next()?.trim().to_string();

            Some(CommitInfo {
                change_id: commit_id.clone(),
                commit_id,
                description,
                author,
                timestamp,
            })
        })
        .collect();

    Ok(commits)
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

    use super::{commits_in_range, current_head_sha, parent_shas};

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
    fn parent_shas_returns_three_parents_for_octopus_merge_commit() -> Result<()> {
        let repo = TempRepo::create()?;
        let head = git(&repo.path, &["rev-parse", "HEAD"])?;

        let parents = parent_shas(&repo.path, &head)?;

        assert_eq!(parents.len(), 3);
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
}