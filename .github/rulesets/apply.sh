#!/bin/sh
# Creates the rulesets in this folder. A private repository needs GitHub Pro.
cd "$(dirname "$0")" || exit 1
for f in *.json; do
    gh api "repos/{owner}/{repo}/rulesets" --input "$f" --jq '.name + ": " + .enforcement' || exit 1
done
