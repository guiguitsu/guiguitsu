# jj-lib API Reference

Crate: `jj-lib`. Unstable API. Add as Cargo dependency.

## Load a Workspace & Repo

```rust
use jj_lib::workspace::Workspace;
use jj_lib::repo::{ReadonlyRepo, RepoLoader, StoreFactories};
use jj_lib::settings::UserSettings;

let workspace = Workspace::load(&user_settings, &path, &StoreFactories::default(), &working_copy_factories)?;
let repo: Arc<ReadonlyRepo> = workspace.repo_loader().load_at_head().await?;
```

`RepoLoader` also has `load_at(operation)` to load at a specific operation.

## Query Commits (Revsets)

```rust
use jj_lib::revset::{RevsetExpression, RevsetIteratorExt};

let expr = RevsetExpression::all();
// Combinators: .parents(), .ancestors(), .children(), .descendants(),
//   .heads(), .roots(), .latest(count)
// Set ops: .union(&other), .intersection(&other), .minus(&other)
// Constructors: ::commit(id), ::commits(ids), ::symbol(string), ::visible_heads(), ::root()

let revset = expr.evaluate(&*repo)?;
// Iterate commit IDs
for commit_id in revset.iter().commit_ids() {
    let commit = repo.store().get_commit(&commit_id)?;
}
```

## Read Commit Data

```rust
use jj_lib::commit::Commit;

commit.id() -> &CommitId
commit.change_id() -> &ChangeId
commit.parent_ids() -> &[CommitId]
commit.description() -> &str
commit.author() -> &Signature        // .name, .email, .timestamp
commit.committer() -> &Signature
commit.tree() -> MergedTree
commit.is_empty(&repo) -> Result<bool>
commit.has_conflict() -> bool
```

## Transactions (Mutations)

All writes go through transactions:

```rust
use jj_lib::transaction::Transaction;

let mut tx = repo.start_transaction();
let mut_repo = tx.repo_mut();

// Create a new commit
let builder = mut_repo.new_commit(parent_ids, tree);
builder.set_description("msg").write().await?;

// Rewrite an existing commit
let builder = mut_repo.rewrite_commit(&old_commit);
builder.set_description("new msg").write().await?;

// Abandon a commit
mut_repo.record_abandoned_commit(&old_commit);

// Always rebase descendants after rewrites
mut_repo.rebase_descendants().await?;

// Commit the transaction
let new_repo = tx.commit("operation description").await?;
```

## Diffs Between Trees

```rust
use jj_lib::merged_tree::MergedTree;
use jj_lib::matchers::EverythingMatcher;

let tree1 = commit1.tree();
let tree2 = commit2.tree();
let diff_stream = tree1.diff_stream(&tree2, &EverythingMatcher);
// Each item yields (RepoPathBuf, Merge<Option<TreeValue>>, Merge<Option<TreeValue>>)
```

## Bookmarks (Branches)

```rust
let view = repo.view();

// Iterate all local bookmarks
for (name, target) in view.local_bookmarks() { ... }

// Bookmarks pointing at a specific commit
for (name, target) in view.local_bookmarks_for_commit(&commit_id) { ... }

// Tags
for (name, target) in view.local_tags() { ... }

// Working copy commit
let wc_id = view.get_wc_commit_id(&workspace_name);

// Heads
let heads: &HashSet<CommitId> = view.heads();
```

## Rebase / Rewrite

```rust
use jj_lib::rewrite::{CommitRewriter, RebaseOptions, rebase_commit_with_options};

let rewriter = CommitRewriter::new(tx.repo_mut(), old_commit, new_parent_ids);
// rewriter.rebase().await? -> CommitBuilder (rebase with new parents, merge tree)
// rewriter.reparent() -> CommitBuilder (change parents, keep exact tree)
// rewriter.abandon() -> mark commit abandoned

let builder = rewriter.rebase().await?;
builder.write().await?;
tx.repo_mut().rebase_descendants().await?;
```

## Alt: CLI JSON Output

If not using Rust, shell out to `jj` with `-T 'json(self) ++ "\n"'`:

```
jj log -T 'json(self) ++ "\n"' --no-graph
jj show -T 'json(self)'
jj evolog -T 'json(self)' --no-graph
jj bookmark list -T 'json(self) ++ "\n"'
jj op log -T 'json(self) ++ "\n"'
jj config list -T 'json(self) ++ "\n"'
```

Returns JSON per line with fields: `commit_id`, `parents`, `change_id`, `description`, `author` (`name`, `email`, `timestamp`), `committer`.

Mutation commands (`jj new`, `jj rebase`, `jj squash`, etc.) don't have JSON output - check exit codes then re-query. `jj diff` has `--summary`, `--name-only` but no JSON.
