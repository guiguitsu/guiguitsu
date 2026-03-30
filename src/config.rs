use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::git_utils;

pub const FILE_NAME: &str = ".guiguitsu.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub workspace_branch: String,
    pub workspace_remote: String,
    pub trunk: String,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_commit: Option<String>,
}

impl Config {
    pub fn path(repo_path: &Path) -> PathBuf {
        repo_path.join(FILE_NAME)
    }

    pub fn load(repo_path: &Path) -> Result<Self> {
        let path = Self::path(repo_path);
        if !path.is_file() {
            bail!(
                "missing {} in repo. Run: guiguitsu create-config --workspace-branch=<branch> --workspace-remote=<remote> --trunk=<main>",
                FILE_NAME
            );
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: Self = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        config.validate(repo_path)?;
        Ok(config)
    }

    pub fn save(&self, repo_path: &Path) -> Result<()> {
        let path = Self::path(repo_path);
        let contents = serde_json::to_string_pretty(self).context("failed to serialize config")?;
        fs::write(&path, format!("{contents}\n"))
            .with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn validate(&self, repo_path: &Path) -> Result<()> {
        git_utils::ensure_remote_exists(repo_path, &self.workspace_remote)
    }

    pub fn base_ref(&self) -> String {
        format!("{}/{}", self.workspace_remote, self.trunk)
    }
}