#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

# Waits for the main-branch CI run of SOURCE_COMMIT and fails unless it
# succeeded, so a release publishes only a commit CI passed on main.
set -eo pipefail

maximum_attempts=160
api_failure_attempts=0
missing_attempts=0
for ((attempt = 1; attempt <= maximum_attempts; ++attempt)); do
  if ! response=$(gh api \
      --method GET \
      "repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml/runs" \
      -f branch="$DEFAULT_BRANCH" \
      -f event=push \
      -f head_sha="$SOURCE_COMMIT" \
      -f per_page=100); then
    ((api_failure_attempts += 1))
    if [[ "$api_failure_attempts" -ge 6 ]]; then
      echo "Could not query main CI after $api_failure_attempts attempts"
      exit 1
    fi
    echo "GitHub API request failed ($attempt/$maximum_attempts)"
    sleep 30
    continue
  fi
  api_failure_attempts=0
  run=$(jq -c \
    --arg branch "$DEFAULT_BRANCH" \
    --arg commit "$SOURCE_COMMIT" \
    '.workflow_runs
      | map(select(
          .path == ".github/workflows/ci.yml"
          and .event == "push"
          and .head_branch == $branch
          and .head_sha == $commit
        ))
      | sort_by(.created_at)
      | reverse
      | .[0] // empty' \
    <<< "$response")
  if [[ -z "$run" ]]; then
    ((missing_attempts += 1))
    if [[ "$missing_attempts" -ge 6 ]]; then
      echo "No main CI run exists for $SOURCE_COMMIT"
      exit 1
    fi
    echo "Waiting for CI on $SOURCE_COMMIT ($attempt/$maximum_attempts)"
  else
    missing_attempts=0
    status=$(jq -r '.status' <<< "$run")
    conclusion=$(jq -r '.conclusion // ""' <<< "$run")
    run_url=$(jq -r '.html_url' <<< "$run")
    if [[ "$status" == "completed" ]]; then
      if [[ "$conclusion" == "success" ]]; then
        echo "Using successful main CI run: $run_url"
        printf 'Main CI: [%s](%s)\n' "$run_url" "$run_url" \
          >> "$GITHUB_STEP_SUMMARY"
        exit 0
      fi
      echo "Main CI concluded $conclusion: $run_url"
      exit 1
    fi
    echo "Waiting for main CI ($status): $run_url"
  fi
  sleep 30
done
echo "Timed out waiting for main CI on $SOURCE_COMMIT"
exit 1
