use std::env;
use std::path::Path;

use anyhow::{Result, bail};

use crate::git_utils::run_command;

fn run_jj(repo_path: &Path, args: &[&str]) -> Result<String> {
    run_command("jj", args, Some(repo_path))
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
pub fn rebase_merge_commit(repo_path: &Path, merge_sha: &str, parents: &[String]) -> Result<String> {
    // Capture the change-id before the rebase — it survives the operation unchanged.
    let change_id = run_jj(repo_path, &["log", "-r", merge_sha, "--no-graph", "-T", "change_id"])?;

    let mut args = vec!["rebase", "-r", merge_sha];
    for parent in parents {
        args.push("-d");
        args.push(parent.as_str());
    }
    run_jj(repo_path, &args)?;

    // Look up the new commit SHA via the stable change-id.
    run_jj(repo_path, &["log", "-r", &change_id, "--no-graph", "-T", "commit_id"])
}

pub fn abandon_commit(repo_path: &Path, sha: &str) -> Result<()> {
    run_jj(repo_path, &["abandon", sha, "--ignore-immutable"])?;
    Ok(())
}

/// Move the working copy to a new empty commit on top of `sha`.
/// Returns the SHA of the newly created commit.
pub fn new_at(repo_path: &Path, sha: &str) -> Result<String> {
    run_jj(repo_path, &["new", sha])?;
    run_jj(repo_path, &["log", "-r", "@", "--no-graph", "-T", "commit_id"])
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

pub fn rebase_after(repo_path: &Path, revision: &str, target: &str) -> Result<()> {
    run_jj(repo_path, &["rebase", "-r", revision, "-A", target])?;
    Ok(())
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

    use super::{abandon_commit, create_bookmark, new_at};
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
}
