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

# Root commit
echo "initial" > file.txt
git add file.txt
git commit -m "root commit"
git tag root

# Second commit on main
echo "main line 2" >> file.txt
git add file.txt
git commit -m "main commit 2"

# Third commit on main
echo "main line 3" >> file.txt
git add file.txt
git commit -m "main commit 3"
git tag main3

# Feature branch from root
git checkout -b feature root

echo "feature work 1" > feature.txt
git add feature.txt
git commit -m "feature commit 1"

echo "feature work 2" >> feature.txt
git add feature.txt
git commit -m "feature commit 2"

# Second feature branch from root
git checkout -b feature2 root

echo "feature2 work" > feature2.txt
git add feature2.txt
git commit -m "feature2 commit 1"

# Merge feature into main
git checkout main
git merge --no-ff feature -m "merge feature into main"
git tag merge-commit
