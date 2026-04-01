use std::path::PathBuf;

use anyhow::Result;

use crate::git_utils::{CommitInfo, commits_in_range, current_head_sha, merge_base, parent_shas};
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
    /// Ordered stack names from config — each index corresponds to the same
    /// index in the merge commit's parent list.
    config_stacks: Vec<String>,
}

impl GitStackProvider {
    pub fn new(repo_path: PathBuf, trunk_name: String, config_stacks: Vec<String>) -> Self {
        Self { repo_path, trunk_name, config_stacks }
    }
}

impl StackProvider for GitStackProvider {
    fn get_stacks(&self) -> Result<Vec<StackInfo>> {
        let head_sha = current_head_sha(&self.repo_path)?;

        let mut sha = head_sha;
        let git_parents = loop {
            let parents = parent_shas(&self.repo_path, &sha)?;
            if parents.len() >= 2 {
                break parents;
            }
            match parents.into_iter().next() {
                Some(p) => sha = p,
                None => return Ok(vec![]),
            }
        };

        if self.config_stacks.len() != git_parents.len() {
            bail!(
                "config stacks count ({}) does not match git merge parents count ({})",
                self.config_stacks.len(),
                git_parents.len(),
            );
        }

        // Find the trunk parent SHA using config ordering
        let trunk_index = self.config_stacks.iter()
            .position(|s| s == &self.trunk_name)
            .ok_or_else(|| anyhow::anyhow!("trunk '{}' not found in config stacks", self.trunk_name))?;
        let trunk_sha = &git_parents[trunk_index];

        let mut stacks = Vec::new();
        for (i, name) in self.config_stacks.iter().enumerate() {
            let parent_sha = &git_parents[i];
            if name == &self.trunk_name {
                stacks.push(StackInfo {
                    name: self.trunk_name.clone(),
                    commits: vec![],
                    base_commit_id: trunk_sha.clone(),
                });
            } else {
                let base = merge_base(&self.repo_path, trunk_sha, parent_sha)?;
                let commits = commits_in_range(&self.repo_path, trunk_sha, parent_sha)?;
                stacks.push(StackInfo {
                    name: name.clone(),
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

    use super::{GitStackProvider, StackProvider};

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
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), vec!["workspace".to_string(), "main".to_string()]);

        let stacks = provider.get_stacks()?;

        assert_eq!(stacks.len(), 2);
        Ok(())
    }

    #[test]
    fn get_stacks_returns_correct_stack_names_from_repo1() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), vec!["workspace".to_string(), "main".to_string()]);

        let stacks = provider.get_stacks()?;

        let names: Vec<&str> = stacks.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["workspace", "main"]);
        Ok(())
    }

    #[test]
    fn base_commit_id_is_fork_point() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), vec!["workspace".to_string(), "main".to_string()]);

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
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), vec!["workspace".to_string(), "main".to_string()]);

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
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string(), vec!["workspace".to_string(), "main".to_string()]);

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
            vec!["workspace".to_string(), "main".to_string(), "extra".to_string()],
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
