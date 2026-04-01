#!/bin/bash

set -euo pipefail

repo_path="${1:-}"

if [ -z "$repo_path" ]; then
	echo "Usage: $0 <new-repo-path>"
	exit 1
fi

if [ -e "$repo_path" ]; then
	echo "Path already exists: $repo_path"
	exit 1
fi

mkdir -p "$repo_path"
cd "$repo_path"

git init
git symbolic-ref HEAD refs/heads/main
git config user.name "Test User"
git config user.email "test@example.com"

echo "initial" > file.txt
git add file.txt
git commit -m "initial commit"

# Initialize jj colocated repo
jj git init --colocate

# Create a few commits and bookmarks
jj new main
echo "feature A" > a.txt
jj desc -m "feature A commit"
feature_a_change_id="$(jj log -r @ --no-graph -T 'change_id')"
jj bookmark create feature-a -r @

jj new main
echo "feature B" > b.txt
jj desc -m "feature B commit"
feature_b_change_id="$(jj log -r @ --no-graph -T 'change_id')"
jj bookmark create feature-b -r @

jj new main
echo "feature C" > c.txt
jj desc -m "feature C commit"
jj bookmark create feature-c -r @

# Move working copy away so bookmarks are stable
jj new main
