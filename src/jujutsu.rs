use std::collections::HashMap;
use std::env;
use std::path::Path;

use anyhow::{Result, bail};

use crate::git_utils::run_command;

fn run_jj(repo_path: &Path, args: &[&str]) -> Result<String> {
    run_command("jj", args, Some(repo_path))
}

/// Returns the operation ID of the most recent jj operation.
pub fn current_op_id(repo_path: &Path) -> Result<String> {
    let output = run_jj(repo_path, &["op", "log", "-n", "1", "--no-graph", "-T", "self.id()"])?;
    let id = output.trim().to_string();
    if id.is_empty() {
        bail!("jj op log returned empty output");
    }
    Ok(id)
}

/// Returns true if the string looks like a jj change-id (all lowercase letters, no digits,
/// and at least one letter beyond 'f' — i.e. not a valid hex string).
fn is_change_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| c.is_ascii_lowercase())
        && !s.chars().any(|c| c.is_ascii_digit())
        && s.chars().any(|c| c > 'f')
}

/// Converts a ref to a git SHA-1. If it's already a SHA-1 (hex), returns it as-is.
/// If it looks like a jj change-id, resolves it via `jj show`.
pub fn to_sha1(repo_path: &Path, rev: &str) -> Result<String> {
    if is_change_id(rev) {
        run_jj(repo_path, &["show", "-T", "commit_id", "--no-patch", rev])
    } else {
        Ok(rev.to_string())
    }
}

/// Converts a git SHA-1 to a jj change-id. If it's already a change-id, returns it as-is.
pub fn to_change_id(repo_path: &Path, rev: &str) -> Result<String> {
    if is_change_id(rev) {
        Ok(rev.to_string())
    } else {
        run_jj(repo_path, &["show", "-T", "change_id", "--no-patch", rev])
    }
}

fn jj_config_get(repo_path: &Path, key: &str) -> Result<String> {
    run_jj(repo_path, &["config", "get", key])
}

fn jj_config_set_user(repo_path: &Path, key: &str, value: &str) -> Result<()> {
    run_jj(repo_path, &["config", "set", "--user", key, value])?;
    Ok(())
}

pub fn ensure_user_config(repo_path: &Path) -> Result<()> {
    let name = jj_config_get(repo_path, "user.name").unwrap_or_default();
    let email = jj_config_get(repo_path, "user.email").unwrap_or_default();

    let name_missing = name.is_empty();
    let email_missing = email.is_empty();

    if !name_missing && !email_missing {
        return Ok(());
    }

    let env_name = env::var("GIT_AUTHOR_NAME").unwrap_or_default();
    let env_email = env::var("GIT_AUTHOR_EMAIL").unwrap_or_default();

    if !env_name.is_empty() && !env_email.is_empty() {
        if name_missing {
            jj_config_set_user(repo_path, "user.name", &env_name)?;
        }
        if email_missing {
            jj_config_set_user(repo_path, "user.email", &env_email)?;
        }
        return Ok(());
    }

    bail!(
        "jj user identity is not configured. Run:\n\n  \
         jj config set --user user.name \"Some One\"\n  \
         jj config set --user user.email \"someone@example.com\""
    );
}

/// Rebase the merge commit onto the given parents.
/// Returns the new git commit SHA (the SHA changes because parents changed).
pub fn rebase_merge_commit(repo_path: &Path, merge_sha: &str, parents: &[String], use_source: bool) -> Result<String> {
    // Capture the change-id before the rebase — it survives the operation unchanged.
    let change_id = run_jj(repo_path, &["log", "-r", merge_sha, "--no-graph", "-T", "change_id"])?;

    let flag = if use_source { "-s" } else { "-r" };
    let mut args = vec!["rebase", flag, merge_sha];
    for parent in parents {
        args.push("-d");
        args.push(parent.as_str());
    }
    run_jj(repo_path, &args)?;

    // Look up the new commit SHA via the stable change-id.
    run_jj(repo_path, &["log", "-r", &change_id, "--no-graph", "-T", "commit_id"])
}

/// Rebase the merge commit and all its descendants onto the given parents.
/// Uses `jj rebase -s` (source) so descendants are also rebased.
pub fn rebase_source(repo_path: &Path, revision: &str, parents: &[String]) -> Result<()> {
    rebase_source_impl(repo_path, revision, parents, false)
}

pub fn rebase_source_ignore_immutable(repo_path: &Path, revision: &str, parents: &[String]) -> Result<()> {
    rebase_source_impl(repo_path, revision, parents, true)
}

fn rebase_source_impl(repo_path: &Path, revision: &str, parents: &[String], ignore_immutable: bool) -> Result<()> {
    let mut args = vec!["rebase"];
    if ignore_immutable {
        args.push("--ignore-immutable");
    }
    args.extend_from_slice(&["-s", revision]);
    for parent in parents {
        args.push("-d");
        args.push(parent.as_str());
    }
    run_jj(repo_path, &args)?;
    Ok(())
}

pub fn abandon_commit(repo_path: &Path, sha: &str) -> Result<()> {
    run_jj(repo_path, &["abandon", sha, "--ignore-immutable"])?;
    Ok(())
}

/// Move the working copy to a new empty commit on top of `sha`.
/// Returns the SHA of the newly created commit.
/// Move the working copy to a new empty commit on top of `revision`.
/// Returns the SHA of the newly created commit.
pub fn new_at(repo_path: &Path, revision: &str) -> Result<String> {
    run_jj(repo_path, &["new", revision])?;
    run_jj(repo_path, &["log", "-r", "@", "--no-graph", "-T", "commit_id"])
}

/// Move the working copy to a new empty commit on top of `revision`.
/// Does not look up the new commit's SHA.
pub fn new_only(repo_path: &Path, revision: &str) -> Result<()> {
    run_jj(repo_path, &["new", revision])?;
    Ok(())
}

/// Return commit SHAs of the direct children of `revision`.
fn children_of(repo_path: &Path, revision: &str) -> Result<Vec<String>> {
    let revset = format!("{revision}+");
    let output = run_jj(
        repo_path,
        &["log", "-r", &revset, "--no-graph", "-T", r#"commit_id ++ "\n""#],
    )?;
    Ok(output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Create a new empty commit on top of `revision` without moving @, and
/// return the commit SHA of the newly created child.
pub fn new_no_edit_on(repo_path: &Path, revision: &str) -> Result<String> {
    let before: std::collections::HashSet<String> =
        children_of(repo_path, revision)?.into_iter().collect();
    run_jj(repo_path, &["new", revision, "--no-edit"])?;
    let after = children_of(repo_path, revision)?;
    after
        .into_iter()
        .find(|s| !before.contains(s))
        .ok_or_else(|| anyhow::anyhow!("failed to identify newly created commit on {revision}"))
}

/// Returns the output of `jj resolve --list`, or empty string if no conflicts.
pub fn resolve_list(repo_path: &Path) -> Result<String> {
    match run_jj(repo_path, &["resolve", "--list", "--color=always"]) {
        Ok(output) => Ok(output),
        Err(_) => Ok(String::new()),
    }
}

pub fn status(repo_path: &Path) -> Result<String> {
    run_jj(repo_path, &["status", "--color=always"])
}

pub fn create_bookmark(repo_path: &Path, name: &str, revision: &str) -> Result<()> {
    run_jj(repo_path, &["bookmark", "create", name, "-r", revision])?;
    Ok(())
}

pub fn set_bookmark(repo_path: &Path, name: &str, revision: &str) -> Result<()> {
    run_jj(repo_path, &["bookmark", "set", name, "-r", revision])?;
    Ok(())
}

pub fn create_merge_commit(repo_path: &Path, message: &str, shas: &[&str], do_new: bool) -> Result<()> {
    let mut args = vec!["new", "-m", message];
    args.extend_from_slice(shas);
    run_jj(repo_path, &args)?;
    if do_new {
        run_jj(repo_path, &["new"])?;
    }
    Ok(())
}

pub fn describe_current(repo_path: &Path, message: &str) -> Result<()> {
    run_jj(repo_path, &["desc", "-m", message])?;
    Ok(())
}

pub fn new_after(repo_path: &Path, parent: &str, message: Option<&str>) -> Result<()> {
    let mut args = vec!["new", "-A", parent, "--no-edit"];
    if let Some(msg) = message {
        args.push("-m");
        args.push(msg);
    }
    run_jj(repo_path, &args)?;
    Ok(())
}

pub fn rebase_after(repo_path: &Path, revision: &str, target: &str) -> Result<()> {
    run_jj(repo_path, &["rebase", "-r", revision, "-A", target])?;
    Ok(())
}

/// Returns a list of bookmark names.
/// If `remote` is true, returns remote bookmarks; otherwise returns local bookmarks.
pub fn list_bookmarks(repo_path: &Path, remote: bool) -> Result<Vec<String>> {
    let template = if remote { "remote_bookmarks" } else { "local_bookmarks" };
    let output = run_jj(
        repo_path,
        &["log", "-r", "bookmarks()", "--no-graph", "-T", &format!("{template} ++ \"\\n\"")],
    )?;
    let bookmarks: Vec<String> = output
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && !line.ends_with("@git"))
        .collect();
    Ok(bookmarks)
}

/// Returns a map from commit SHA to bookmark names (local and/or remote).
pub fn bookmarks_by_commit(repo_path: &Path) -> Result<HashMap<String, Vec<String>>> {
    let template = r#"commit_id ++ "\t" ++ local_bookmarks ++ " " ++ remote_bookmarks ++ "\n""#;
    let output = run_jj(
        repo_path,
        &["log", "-r", "bookmarks()", "--no-graph", "-T", template],
    )?;
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (sha, rest) = line.split_once('\t').unwrap_or((line, ""));
        let names: Vec<String> = rest
            .split_whitespace()
            .filter(|s| !s.is_empty() && !s.ends_with("@git"))
            .map(|s| s.to_string())
            .collect();
        if !names.is_empty() {
            map.entry(sha.to_string()).or_default().extend(names);
        }
    }
    Ok(map)
}

/// Resolves a bookmark name to its jj change-id.
pub fn bookmark_to_change_id(repo_path: &Path, bookmark: &str) -> Result<String> {
    run_jj(repo_path, &["log", "-r", bookmark, "--no-graph", "-T", "change_id"])
}

/// Push a bookmark to its remote via `jj git push`.
pub fn git_push_bookmark(repo_path: &Path, bookmark: &str, remote: &str) -> Result<()> {
    run_jj(repo_path, &["git", "push", "--bookmark", bookmark, "--remote", remote])?;
    Ok(())
}

/// Returns true if the given revision is an empty commit (no file changes).
pub fn is_empty_commit(repo_path: &Path, rev: &str) -> Result<bool> {
    let output = run_jj(
        repo_path,
        &["log", "-r", rev, "--no-graph", "-T", r#"if(self.empty(), "empty", "not empty")"#],
    )?;
    Ok(output.trim() == "empty")
}

/// Returns all descendants of `rev` (excluding `rev` itself) as `CommitInfo` structs.
/// Uses `jj log` with the `rev::` revset to find descendants reliably.
pub fn descendants_of(repo_path: &Path, rev: &str) -> Result<Vec<crate::git_utils::CommitInfo>> {
    let revset = format!("{rev}::");
    let template = r#"commit_id ++ "\x1f" ++ change_id ++ "\x1f" ++ description.first_line() ++ "\n""#;
    let output = run_jj(repo_path, &["log", "-r", &revset, "--no-graph", "-T", template])?;

    let rev_sha = to_sha1(repo_path, rev)?;

    let mut result = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.splitn(3, '\x1f');
        let commit_id = fields.next().unwrap_or("").to_string();
        let change_id = fields.next().unwrap_or("").to_string();
        let description = fields.next().unwrap_or("").to_string();
        // Skip the starting commit itself.
        if commit_id == rev_sha {
            continue;
        }
        result.push(crate::git_utils::CommitInfo {
            change_id,
            commit_id,
            description,
            author: String::new(),
            timestamp: String::new(),
            changed_files: Vec::new(),
        });
    }

    Ok(result)
}

pub fn absorb(repo_path: &Path, paths: &[&str]) -> Result<()> {
    let mut args = vec!["absorb"];
    args.extend_from_slice(paths);
    run_jj(repo_path, &args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::{Context, Result, anyhow};

    use std::fs;

    use super::{abandon_commit, bookmark_to_change_id, bookmarks_by_commit, create_bookmark, descendants_of, is_empty_commit, list_bookmarks, new_at};
    use crate::git_utils::{find_commit_by_description, parent_shas, run_command};

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
            path.push(format!("guiguitsu-jj-test-{now}-{}", std::process::id()));

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
    fn abandon_commit_reparents_child_to_grandparent() -> Result<()> {
        let repo = TempRepo::create()?;

        let sha1 = find_commit_by_description(&repo.path, "branch1 commit 1")?;
        let sha2 = find_commit_by_description(&repo.path, "branch1 commit 2")?;
        let sha3 = find_commit_by_description(&repo.path, "branch1 commit 3")?;

        // Precondition: commit 3 is a direct child of commit 2.
        assert_eq!(parent_shas(&repo.path, &sha3)?, vec![sha2.clone()]);

        abandon_commit(&repo.path, &sha2)?;

        // After abandoning commit 2, commit 3 should be reparented onto commit 1.
        let new_sha3 = find_commit_by_description(&repo.path, "branch1 commit 3")?;
        assert_eq!(parent_shas(&repo.path, &new_sha3)?, vec![sha1]);

        Ok(())
    }

    #[test]
    fn new_at_returns_sha_of_new_commit() -> Result<()> {
        let repo = TempRepo::create()?;
        let main_sha = run_command("git", &["rev-parse", "main"], Some(&repo.path))?;

        let new_sha = new_at(&repo.path, &main_sha)?;

        assert!(!new_sha.is_empty());
        assert_eq!(parent_shas(&repo.path, &new_sha)?, vec![main_sha]);

        Ok(())
    }

    #[test]
    fn create_bookmark_creates_local_branch() -> Result<()> {
        let repo = TempRepo::create()?;
        let main_sha = run_command("git", &["rev-parse", "main"], Some(&repo.path))?;

        let new_sha = new_at(&repo.path, &main_sha)?;
        create_bookmark(&repo.path, "my-feature", &new_sha)?;

        let branch_sha = run_command("git", &["rev-parse", "my-feature"], Some(&repo.path))?;
        assert_eq!(branch_sha, new_sha);

        Ok(())
    }

    struct TempBookmarkRepo {
        path: PathBuf,
    }

    impl TempBookmarkRepo {
        fn create() -> Result<Self> {
            let mut path = std::env::temp_dir();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("time went backwards")?
                .as_nanos();
            path.push(format!("guiguitsu-jj-bookmark-test-{now}-{}", std::process::id()));

            let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repos/test_bookmarks.sh");
            let output = Command::new("bash")
                .arg(script)
                .arg(&path)
                .output()
                .context("failed to execute test_bookmarks.sh")?;

            if !output.status.success() {
                return Err(anyhow!(
                    "test_bookmarks.sh failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }

            Ok(Self { path })
        }
    }

    impl Drop for TempBookmarkRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn list_local_bookmarks_returns_created_bookmarks() -> Result<()> {
        let repo = TempBookmarkRepo::create()?;

        let bookmarks = list_bookmarks(&repo.path, false)?;

        assert!(bookmarks.iter().any(|b| b.contains("feature-a")), "expected feature-a in {bookmarks:?}");
        assert!(bookmarks.iter().any(|b| b.contains("feature-b")), "expected feature-b in {bookmarks:?}");
        assert!(bookmarks.iter().any(|b| b.contains("feature-c")), "expected feature-c in {bookmarks:?}");
        assert!(bookmarks.iter().any(|b| b.contains("main")), "expected main in {bookmarks:?}");

        Ok(())
    }

    #[test]
    fn list_remote_bookmarks_excludes_git_tracking_bookmarks() -> Result<()> {
        let repo = TempBookmarkRepo::create()?;

        // Colocated jj repos only have @git tracking bookmarks, which should be filtered out.
        let bookmarks = list_bookmarks(&repo.path, true)?;
        assert!(
            bookmarks.iter().all(|b| !b.ends_with("@git")),
            "expected no @git bookmarks, got {bookmarks:?}"
        );

        Ok(())
    }

    #[test]
    fn bookmarks_by_commit_maps_shas_to_names() -> Result<()> {
        let repo = TempBookmarkRepo::create()?;

        let map = bookmarks_by_commit(&repo.path)?;

        // At least one commit should have a bookmark containing "feature-a".
        let has_feature_a = map.values().any(|names| names.iter().any(|n| n.contains("feature-a")));
        assert!(has_feature_a, "expected a commit with feature-a bookmark in {map:?}");

        // The main bookmark should also appear.
        let has_main = map.values().any(|names| names.iter().any(|n| n == "main"));
        assert!(has_main, "expected a commit with main bookmark in {map:?}");

        Ok(())
    }

    #[test]
    fn bookmark_to_change_id_resolves_bookmark() -> Result<()> {
        let repo = TempBookmarkRepo::create()?;

        let change_id = bookmark_to_change_id(&repo.path, "feature-a")?;
        assert!(!change_id.is_empty(), "expected non-empty change-id for feature-a");

        // The change-id should be all lowercase letters (jj change-id format).
        assert!(
            change_id.chars().all(|c| c.is_ascii_lowercase()),
            "expected all lowercase letters in change-id, got: {change_id}"
        );

        Ok(())
    }

    struct TempDescendantsRepo {
        path: PathBuf,
    }

    impl TempDescendantsRepo {
        fn create() -> Result<Self> {
            let mut path = std::env::temp_dir();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("time went backwards")?
                .as_nanos();
            path.push(format!("guiguitsu-jj-descendants-test-{now}-{}", std::process::id()));

            let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repos/test_descendants.sh");
            let output = Command::new("bash")
                .arg(script)
                .arg(&path)
                .output()
                .context("failed to execute test_descendants.sh")?;

            if !output.status.success() {
                return Err(anyhow!(
                    "test_descendants.sh failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }

            Ok(Self { path })
        }
    }

    impl Drop for TempDescendantsRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn descendants_of_returns_children_and_grandchildren() -> Result<()> {
        let repo = TempDescendantsRepo::create()?;

        let merge_change_id = fs::read_to_string(repo.path.join(".merge_change_id"))?.trim().to_string();

        let descendants = descendants_of(&repo.path, &merge_change_id)?;

        // Expect 2 descendants: the child and grandchild commits.
        assert_eq!(descendants.len(), 2, "expected 2 descendants, got: {descendants:?}");
        assert!(
            descendants.iter().any(|c| c.description == "unstacked child commit"),
            "child should be a descendant"
        );
        assert!(
            descendants.iter().any(|c| c.description == "unstacked grandchild commit"),
            "grandchild should be a descendant"
        );

        Ok(())
    }

    #[test]
    fn descendants_of_returns_empty_at_tip() -> Result<()> {
        let repo = TempDescendantsRepo::create()?;

        let merge_change_id = fs::read_to_string(repo.path.join(".merge_change_id"))?.trim().to_string();
        let descendants = descendants_of(&repo.path, &merge_change_id)?;

        // The grandchild is the last real commit; get its change-id from the results.
        let grandchild = descendants.iter()
            .find(|c| c.description == "unstacked grandchild commit")
            .expect("grandchild should exist");

        let tip_descendants = descendants_of(&repo.path, &grandchild.change_id)?;
        assert!(tip_descendants.is_empty(), "tip commit should have no descendants");
        Ok(())
    }

    #[test]
    fn descendants_of_excludes_merge_commit_itself() -> Result<()> {
        let repo = TempDescendantsRepo::create()?;

        let merge_change_id = fs::read_to_string(repo.path.join(".merge_change_id"))?.trim().to_string();

        let descendants = descendants_of(&repo.path, &merge_change_id)?;

        assert!(
            !descendants.iter().any(|c| c.description == "Special workspace merge commit"),
            "merge commit itself should not appear in descendants"
        );

        Ok(())
    }

    #[test]
    fn is_empty_commit_returns_true_for_empty() -> Result<()> {
        let repo = TempDescendantsRepo::create()?;

        let merge_change_id = fs::read_to_string(repo.path.join(".merge_change_id"))?.trim().to_string();

        // Create an empty commit on top of the merge.
        let empty_sha = new_at(&repo.path, &merge_change_id)?;

        assert!(is_empty_commit(&repo.path, &empty_sha)?, "commit created with jj new should be empty");
        Ok(())
    }

    #[test]
    fn is_empty_commit_returns_false_for_non_empty() -> Result<()> {
        let repo = TempDescendantsRepo::create()?;

        let merge_change_id = fs::read_to_string(repo.path.join(".merge_change_id"))?.trim().to_string();

        // Create a commit on top of the merge and add a file to make it non-empty.
        new_at(&repo.path, &merge_change_id)?;
        fs::write(repo.path.join("new_file.txt"), "content")?;
        // jj auto-snapshots on next command, rewriting @ with the new file.
        // Use @ to refer to the current working copy after snapshot.
        assert!(!is_empty_commit(&repo.path, "@")?, "commit with a new file should not be empty");
        Ok(())
    }
}
