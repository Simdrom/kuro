#!/bin/sh
# Creates the rulesets in this folder that the repository lacks, by name; it changes none
# already there. A private repository needs GitHub Pro.
# The checks named in develop-checks.json are the names of jobs in ci.yml: rename both together.
cd "$(dirname "$0")" || exit 1
existing=$(gh api "repos/{owner}/{repo}/rulesets" --jq '.[].name') || exit 1
for f in *.json; do
    name=$(sed -n 's/^  "name": "\(.*\)",$/\1/p' "$f")
    if printf '%s\n' "$existing" | grep -qxF "$name"; then
        echo "$name: already there"
        continue
    fi
    gh api "repos/{owner}/{repo}/rulesets" --input "$f" --jq '.name + ": " + .enforcement' || exit 1
done
