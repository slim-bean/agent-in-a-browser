#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/publish-codex-upstream.sh [--sha <commit>] [--tag-only] [--dry-run]

Publishes the canonical runtime/codex-upstream carrying commit by:
  1. pushing an immutable keepalive tag for the target commit
  2. force-updating the configured branch (edge-agent) unless --tag-only is set

Options:
  --sha <commit>  Publish/tag a specific reachable commit instead of submodule HEAD
  --tag-only      Only push the immutable keepalive tag
  --dry-run       Print commands without executing them
  -h, --help      Show this help
EOF
}

repo_root=$(git rev-parse --show-toplevel)
gitmodules="$repo_root/.gitmodules"
submodule_path="runtime/codex-upstream"
submodule_dir="$repo_root/$submodule_path"
remote="origin"

target_sha=""
tag_only=0
dry_run=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sha)
      target_sha="${2:-}"
      if [[ -z "$target_sha" ]]; then
        echo "error: --sha requires a commit" >&2
        exit 1
      fi
      shift 2
      ;;
    --tag-only)
      tag_only=1
      shift
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

submodule_url=$(git config -f "$gitmodules" --get "submodule.$submodule_path.url")
submodule_branch=$(git config -f "$gitmodules" --get "submodule.$submodule_path.branch")

if [[ -z "$submodule_url" || -z "$submodule_branch" ]]; then
  echo "error: failed to resolve $submodule_path url/branch from .gitmodules" >&2
  exit 1
fi

if [[ -z "$target_sha" ]]; then
  target_sha=$(git -C "$submodule_dir" rev-parse HEAD)
fi

git -C "$submodule_dir" cat-file -e "${target_sha}^{commit}"

tag_name="codex-upstream-$target_sha"

run() {
  if (( dry_run )); then
    printf '[dry-run] %q' "$1"
    shift
    for arg in "$@"; do
      printf ' %q' "$arg"
    done
    printf '\n'
  else
    "$@"
  fi
}

existing_tag_sha=$(git -C "$submodule_dir" ls-remote --tags "$remote" "refs/tags/$tag_name" | awk '{print $1}')
if [[ -n "$existing_tag_sha" && "$existing_tag_sha" != "$target_sha" ]]; then
  echo "error: remote tag $tag_name already exists at $existing_tag_sha" >&2
  exit 1
fi

if git -C "$submodule_dir" rev-parse -q --verify "refs/tags/$tag_name" >/dev/null; then
  local_tag_sha=$(git -C "$submodule_dir" rev-parse "refs/tags/$tag_name")
  if [[ "$local_tag_sha" != "$target_sha" ]]; then
    echo "error: local tag $tag_name points at $local_tag_sha, expected $target_sha" >&2
    exit 1
  fi
else
  run git -C "$submodule_dir" tag "$tag_name" "$target_sha"
fi

run git -C "$submodule_dir" push "$remote" "refs/tags/$tag_name"

if (( ! tag_only )); then
  run git -C "$submodule_dir" push "$remote" "HEAD:$submodule_branch" --force-with-lease
fi

echo "published $submodule_path:"
echo "  url:    $submodule_url"
echo "  branch: $submodule_branch"
echo "  sha:    $target_sha"
echo "  tag:    $tag_name"
