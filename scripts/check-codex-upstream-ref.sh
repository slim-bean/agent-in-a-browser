#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
gitmodules="$repo_root/.gitmodules"
submodule_path="runtime/codex-upstream"

submodule_url=$(git config -f "$gitmodules" --get "submodule.$submodule_path.url")
submodule_branch=$(git config -f "$gitmodules" --get "submodule.$submodule_path.branch")

if [[ -z "$submodule_url" || -z "$submodule_branch" ]]; then
  echo "error: failed to resolve $submodule_path url/branch from .gitmodules" >&2
  exit 1
fi

actual_sha=$(git ls-tree HEAD "$submodule_path" | awk '{print $3}')
expected_sha=$(git ls-remote "$submodule_url" "refs/heads/$submodule_branch" | awk '{print $1}')

if [[ -z "$actual_sha" || -z "$expected_sha" ]]; then
  echo "error: failed to resolve current or expected $submodule_path SHA" >&2
  exit 1
fi

if [[ "$actual_sha" != "$expected_sha" ]]; then
  echo "error: $submodule_path is pinned to stale SHA" >&2
  echo "  actual:   $actual_sha" >&2
  echo "  expected: $expected_sha (refs/heads/$submodule_branch)" >&2
  echo "Update the submodule pointer before merging or building against main." >&2
  exit 1
fi

keepalive_tag="refs/tags/codex-upstream-$actual_sha"
tag_sha=$(git ls-remote "$submodule_url" "$keepalive_tag" | awk '{print $1}')
if [[ "$tag_sha" != "$actual_sha" ]]; then
  echo "error: missing keepalive tag for published $submodule_path SHA" >&2
  echo "  expected tag: codex-upstream-$actual_sha" >&2
  echo "Publish the submodule via scripts/publish-codex-upstream.sh before merging." >&2
  exit 1
fi

echo "$submodule_path pin OK:"
echo "  branch tip: $submodule_branch -> $expected_sha"
echo "  keepalive:  codex-upstream-$actual_sha"
