use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Result;
use slint::ComponentHandle;

use crate::cli;
use crate::config::Config;
use crate::models;
use crate::stacks::{GitStackProvider, StackProvider};
use crate::{App, git_utils, jujutsu};

pub fn run_app(repo_path: PathBuf, print_merge_command: bool, _verbose: bool) -> Result<()> {
    git_utils::validate_startup_requirements(&repo_path)?;
    let config = Config::load(&repo_path)?;
    let config_stacks: Vec<String> = config.stacks.iter().map(|s| s.name.clone()).collect();
    let merge_commit_ref = config.merge_commit.clone();
    let provider = GitStackProvider::new(repo_path.clone(), config.trunk.clone(), config_stacks.clone(), merge_commit_ref.clone());
    let stacks = provider.get_stacks()?;

    if print_merge_command {
        let merge_ref = config.merge_commit.as_ref()
            .ok_or_else(|| anyhow::anyhow!("no merge_commit in config"))?;
        let merge_sha = jujutsu::to_sha1(&repo_path, merge_ref)?;
        let parents = git_utils::parent_shas(&repo_path, &merge_sha)?;

        let trunk_jj_ref = format!("{}@{}", config.trunk, config.workspace_remote);
        let trunk_index = config.stacks.iter().position(|s| s.name == config.trunk)
            .ok_or_else(|| anyhow::anyhow!("trunk '{}' not found in config stacks", config.trunk))?;

        let mut cmd = format!("jj rebase -s {}", merge_ref);
        for (i, parent_sha) in parents.iter().enumerate() {
            if i == trunk_index {
                cmd.push_str(&format!(" -d {}", trunk_jj_ref));
            } else {
                let change_id = jujutsu::to_change_id(&repo_path, parent_sha)?;
                cmd.push_str(&format!(" -d {}", change_id));
            }
        }
        println!("{cmd}");
        return Ok(());
    }

    let model = models::build_stacks_model(&stacks);
    let app = App::new()?;
    app.set_stacks(model);

    let reload: Rc<dyn Fn()> = {
        let app_weak = app.as_weak();
        let repo_path = repo_path.clone();
        let config_stacks = config_stacks.clone();
        let merge_commit_ref = merge_commit_ref.clone();
        let trunk = config.trunk.clone();
        Rc::new(move || {
            let app = match app_weak.upgrade() { Some(a) => a, None => return };
            let provider = GitStackProvider::new(repo_path.clone(), trunk.clone(), config_stacks.clone(), merge_commit_ref.clone());
            match provider.get_stacks() {
                Ok(stacks) => app.set_stacks(models::build_stacks_model(&stacks)),
                Err(e) => eprintln!("failed to reload stacks: {e}"),
            }
        })
    };

    app.on_abandon_commit({
        let reload = Rc::clone(&reload);
        let repo_path = repo_path.clone();
        move |commit_id| {
            match jujutsu::abandon_commit(&repo_path, &commit_id) {
                Ok(()) => reload(),
                Err(e) => eprintln!("failed to abandon {commit_id}: {e}"),
            }
        }
    });

    app.on_drop_onto_commit(|dragged_id, target_id| {
        println!("[drag] drop onto: {} -> {}", dragged_id, target_id);
    });

    app.on_drop_between_commits(|dragged_id, before_id| {
        if before_id.is_empty() {
            println!("[drag] drop between: {} -> top of stack", dragged_id);
        } else {
            println!("[drag] drop between: {} -> after {}", dragged_id, before_id);
        }
    });

    app.on_refresh({
        let reload = Rc::clone(&reload);
        move || reload()
    });

    app.on_add_branch({
        let repo_path = repo_path.clone();
        let reload = Rc::clone(&reload);
        move |branch_name| {
            if let Err(e) = cli::apply_branch(repo_path.clone(), branch_name.to_string()) {
                eprintln!("apply_branch failed: {e}");
            } else {
                reload();
            }
        }
    });

    let app_weak = app.as_weak();
    let repo_path_for_diff = repo_path.clone();
    app.on_select_commit(move |commit_hash| {
        let app = app_weak.unwrap();
        match git_utils::get_commit_diff(&repo_path_for_diff, &commit_hash) {
            Ok(diff_lines) => {
                app.set_diff_lines(models::build_diff_model(&diff_lines));
            }
            Err(e) => {
                eprintln!("failed to get diff for {commit_hash}: {e}");
            }
        }
    });

    app.run()?;
    Ok(())
}
