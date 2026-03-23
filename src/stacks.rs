use std::path::PathBuf;

use anyhow::Result;

use crate::git_utils::{CommitInfo, branch_name_for, commits_in_range, current_head_sha, is_ancestor, parent_shas};

pub struct StackInfo {
    pub name: String,
    pub commits: Vec<CommitInfo>,
    pub base_commit_id: String,
}

impl StackInfo {
    pub fn head_commit_id(&self) -> Option<&str> {
        self.commits.first().map(|c| c.commit_id.as_str())
    }
}

pub trait StackProvider {
    fn get_stacks(&self) -> Result<Vec<StackInfo>>;
}

pub struct GitStackProvider {
    repo_path: PathBuf,
    trunk: String,
}

impl GitStackProvider {
    pub fn new(repo_path: PathBuf, trunk: String) -> Self {
        Self { repo_path, trunk }
    }
}

impl StackProvider for GitStackProvider {
    fn get_stacks(&self) -> Result<Vec<StackInfo>> {
        let head_sha = current_head_sha(&self.repo_path)?;

        let mut sha = head_sha;
        let parents = loop {
            let parents = parent_shas(&self.repo_path, &sha)?;
            if parents.len() >= 2 {
                break parents;
            }
            match parents.into_iter().next() {
                Some(p) => sha = p,
                None => return Ok(vec![]),
            }
        };

        let trunk_parent = parents
            .iter()
            .find(|p| is_ancestor(&self.repo_path, p, &self.trunk).unwrap_or(false))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no trunk parent found among merge commit parents"))?;

        let mut stacks = Vec::new();

        for branch_parent in parents.iter().filter(|p| *p != &trunk_parent) {
            let name = branch_name_for(&self.repo_path, branch_parent)
                .unwrap_or_else(|_| branch_parent[..branch_parent.len().min(8)].to_string());
            let commits = commits_in_range(&self.repo_path, &trunk_parent, branch_parent)?;
            stacks.push(StackInfo { name, commits, base_commit_id: trunk_parent.clone() });
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
    fn get_stacks_returns_two_stacks_from_repo1() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string());

        let stacks = provider.get_stacks()?;

        assert_eq!(stacks.len(), 1);
        Ok(())
    }

    #[test]
    fn get_stacks_returns_correct_stack_names_from_repo1() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string());

        let stacks = provider.get_stacks()?;

        let names: Vec<&str> = stacks.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["workspace"]);
        Ok(())
    }

    #[test]
    fn base_commit_id_is_tip_of_trunk() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string());

        let stacks = provider.get_stacks()?;
        let stack = &stacks[0];

        let main_tip = Command::new("git")
            .args(["rev-parse", "main"])
            .current_dir(&repo.path)
            .output()
            .context("git rev-parse main")?;
        let expected = String::from_utf8_lossy(&main_tip.stdout).trim().to_string();

        assert_eq!(stack.base_commit_id, expected);
        Ok(())
    }

    #[test]
    fn head_commit_id_is_first_commit_in_stack() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone(), "main".to_string());

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
}
