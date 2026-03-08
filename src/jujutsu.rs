use std::env;
use std::path::Path;

use anyhow::{Result, bail};

use crate::git_utils::run_command;

fn run_jj(repo_path: &Path, args: &[&str]) -> Result<String> {
    run_command("jj", args, Some(repo_path))
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

pub fn create_merge_commit(repo_path: &Path, message: &str, shas: &[&str], do_new: bool) -> Result<()> {
    let mut args = vec!["new", "-m", message];
    args.extend_from_slice(shas);
    run_jj(repo_path, &args)?;
    if do_new {
        run_jj(repo_path, &["new"])?;
    }
    Ok(())
}
