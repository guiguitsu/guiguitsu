use std::path::PathBuf;

use anyhow::Result;

use crate::config;
use crate::git_utils::{self, CommitInfo, commits_in_range, current_head_sha, merge_base, parent_shas};
use anyhow::bail;

pub struct StackInfo {
    pub name: String,
    pub commits: Vec<CommitInfo>,
    pub base_commit_id: String,
}

impl StackInfo {
    pub fn head_commit_id(&self) -> Option<&str> {
        self.commits.first().map(|c| c.commit_id.as_str())
    }

    pub fn root_commit_id(&self) -> Option<&str> {
        self.commits.last().map(|c| c.commit_id.as_str())
    }
}

pub trait StackProvider {
    fn get_stacks(&self) -> Result<Vec<StackInfo>>;
}

pub struct GitStackProvider {
    repo_path: PathBuf,
    trunk_name: String,
    remote: String,
    /// Ordered stack entries from config.
    config_stacks: Vec<config::StackEntry>,
    /// Optional merge commit ref from config (change-id or SHA).
    merge_commit_ref: Option<String>,
}

impl GitStackProvider {
    pub fn new(repo_path: PathBuf, trunk_name: String, remote: String, config_stacks: Vec<config::StackEntry>, merge_commit_ref: Option<String>) -> Self {
        Self { repo_path, trunk_name, remote, config_stacks, merge_commit_ref }
    }
}

impl StackProvider for GitStackProvider {
    fn get_stacks(&self) -> Result<Vec<StackInfo>> {
        let git_parents = if let Some(ref merge_ref) = self.merge_commit_ref {
            let merge_sha = crate::jujutsu::to_sha1(&self.repo_path, merge_ref)?;
            parent_shas(&self.repo_path, &merge_sha)?
        } else {
            // Fallback: walk from HEAD to find the first merge commit.
            let head_sha = current_head_sha(&self.repo_path)?;
            let mut sha = head_sha;
            loop {
                let parents = parent_shas(&self.repo_path, &sha)?;
                if parents.len() >= 2 {
                    break parents;
                }
                match parents.into_iter().next() {
                    Some(p) => sha = p,
                    None => return Ok(vec![]),
                }
            }
        };

        if self.config_stacks.len() != git_parents.len() {
            bail!(
                "config stacks count ({}) does not match git merge parents count ({})",
                self.config_stacks.len(),
                git_parents.len(),
            );
        }

        // Build a mapping from config index → parent index by resolving branch refs.
        // Default: positional (config_stacks[i] → git_parents[i]).
        let mut parent_index_for: Vec<usize> = (0..self.config_stacks.len()).collect();

        let t_branch_res = std::time::Instant::now();
        for (ci, entry) in self.config_stacks.iter().enumerate() {
            let branch_ref = if entry.name == self.trunk_name {
                // For trunk, match via remote ref
                format!("refs/remotes/{}/{}", self.remote, self.trunk_name)
            } else {
                match &entry.local_branch {
                    Some(b) => format!("refs/heads/{b}"),
                    None => continue, // keep positional fallback
                }
            };
            let Ok(branch_sha) = git_utils::resolve_ref(&self.repo_path, &branch_ref) else {
                continue; // branch doesn't exist yet, keep positional fallback
            };
            for (pi, parent) in git_parents.iter().enumerate() {
                if parent == &branch_sha
                    || git_utils::is_ancestor(&self.repo_path, parent, &branch_sha).unwrap_or(false)
                {
                    parent_index_for[ci] = pi;
                    break;
                }
            }
        }
        if crate::verbose() {
            eprintln!("[debug:stacks] branch resolution: {:.2?}", t_branch_res.elapsed());
        }

        let trunk_index = self.config_stacks.iter()
            .position(|s| s.name == self.trunk_name)
            .ok_or_else(|| anyhow::anyhow!("trunk '{}' not found in config stacks", self.trunk_name))?;
        let trunk_sha = &git_parents[parent_index_for[trunk_index]];

        let mut stacks = Vec::new();
        for (i, entry) in self.config_stacks.iter().enumerate() {
            let parent_sha = &git_parents[parent_index_for[i]];
            if entry.name == self.trunk_name {
                stacks.push(StackInfo {
                    name: self.trunk_name.clone(),
                    commits: vec![],
                    base_commit_id: trunk_sha.clone(),
                });
            } else {
                let t_merge_base = std::time::Instant::now();
                let base = merge_base(&self.repo_path, trunk_sha, parent_sha)?;
                let elapsed_merge_base = t_merge_base.elapsed();
                let t_range = std::time::Instant::now();
                let mut commits = commits_in_range(&self.repo_path, trunk_sha, parent_sha)?;
                let elapsed_range = t_range.elapsed();
                let commit_count = commits.len();
                let t_cid = std::time::Instant::now();
                for commit in &mut commits {
                    if let Ok(cid) = crate::jujutsu::to_change_id(&self.repo_path, &commit.commit_id) {
                        commit.change_id = cid;
                    }
                }
                if crate::verbose() {
                    eprintln!(
                        "[debug:stacks] '{}': merge_base {:.2?}, commits_in_range {:.2?} ({} commits), to_change_id {:.2?}",
                        entry.name, elapsed_merge_base, elapsed_range, commit_count, t_cid.elapsed()
                    );
                }
                stacks.push(StackInfo {
                    name: entry.name.clone(),
                    commits,
                    base_commit_id: base,
                });
            }
        }

        Ok(stacks)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::{Context, Result, anyhow};

    use crate::config::StackEntry;
    use super::{GitStackProvider, StackProvider};

    fn make_stack_entries(names: &[&str]) -> Vec<StackEntry> {
        names.iter().map(|n| StackEntry {
            name: n.to_string(),
            local_branch: if *n == "main" { None } else { Some(n.to_string()) },
            remote_branch: Some(format!("{n}@origin")),
        }).collect()
    }

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
            path.push(format!("guiguitsu-stacks-test-{now}-{}", std::process::id()));

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
    fn get_stacks_returns_workspace_and_trunk_from_repo1() -> Result<()> {
        let repo = TempRepo::create()?;
        // repo1 has 2 merge parents (workspace + main), so 0 extra stack entries
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), "origin".to_string(), make_stack_entries(&["workspace", "main"]), None);

        let stacks = provider.get_stacks()?;

        assert_eq!(stacks.len(), 2);
        Ok(())
    }

    #[test]
    fn get_stacks_returns_correct_stack_names_from_repo1() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), "origin".to_string(), make_stack_entries(&["workspace", "main"]), None);

        let stacks = provider.get_stacks()?;

        let names: Vec<&str> = stacks.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["workspace", "main"]);
        Ok(())
    }

    #[test]
    fn base_commit_id_is_fork_point() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), "origin".to_string(), make_stack_entries(&["workspace", "main"]), None);

        let stacks = provider.get_stacks()?;
        let stack = &stacks[0];

        let mb = Command::new("git")
            .args(["merge-base", "main", "workspace"])
            .current_dir(&repo.path)
            .output()
            .context("git merge-base")?;
        let expected = String::from_utf8_lossy(&mb.stdout).trim().to_string();

        assert_eq!(stack.base_commit_id, expected);
        Ok(())
    }

    #[test]
    fn head_commit_id_is_first_commit_in_stack() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), "origin".to_string(), make_stack_entries(&["workspace", "main"]), None);

        let stacks = provider.get_stacks()?;
        let stack = &stacks[0];

        let head = stack.head_commit_id().expect("stack should not be empty");
        assert_eq!(head, stack.commits.first().unwrap().commit_id);
        Ok(())
    }

    #[test]
    fn head_commit_id_returns_none_for_empty_commits() {
        let stack = super::StackInfo {
            name: "empty".to_string(),
            commits: vec![],
            base_commit_id: "abc123".to_string(),
        };
        assert!(stack.head_commit_id().is_none());
    }

    #[test]
    fn root_commit_id_is_last_commit_in_stack() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), "origin".to_string(), make_stack_entries(&["workspace", "main"]), None);

        let stacks = provider.get_stacks()?;
        let stack = &stacks[0];

        let root = stack.root_commit_id().expect("stack should not be empty");
        assert_eq!(root, stack.commits.last().unwrap().commit_id);
        Ok(())
    }

    #[test]
    fn root_commit_id_returns_none_for_empty_commits() {
        let stack = super::StackInfo {
            name: "empty".to_string(),
            commits: vec![],
            base_commit_id: "abc123".to_string(),
        };
        assert!(stack.root_commit_id().is_none());
    }

    #[test]
    fn get_stacks_errors_on_stack_count_mismatch() -> Result<()> {
        let repo = TempRepo::create()?;
        // The repo has 2 merge parents, but we pass 3 config stacks — should error.
        let provider = GitStackProvider::new(
            repo.path.clone(),
            "main".to_string(),
            "origin".to_string(),
            make_stack_entries(&["workspace", "main", "extra"]),
            None,
        );

        match provider.get_stacks() {
            Err(err) => assert!(
                err.to_string().contains("does not match"),
                "unexpected error: {err}"
            ),
            Ok(_) => panic!("expected stack count mismatch error"),
        }
        Ok(())
    }
}
