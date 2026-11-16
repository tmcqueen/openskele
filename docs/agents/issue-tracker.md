# Issue tracker: GitHub

Issues and specs live in GitHub Issues for `tmcqueen/openskele`. Use the `gh` CLI for tracker operations.

## Conventions

- Create: `gh issue create --title "..." --body "..."`. Use a heredoc for multiline bodies.
- Read: `gh issue view <number> --json number,title,body,labels,comments`.
- List: `gh issue list --state open --json number,title,body,labels,comments`, with appropriate label and state filters.
- Comment: `gh issue comment <number> --body "..."`.
- Apply or remove labels: `gh issue edit <number> --add-label "..."` or `--remove-label "..."`.
- Close: post the explanation with `gh issue comment`, then use `gh issue close <number>`. Do not rely on `gh issue close --comment` for an already-closed issue: it can drop the comment.

Infer the repository from the git remote. Before the first issue creation in a session, verify the target with `gh repo view --json nameWithOwner`. File issues only in the user's repository; confirm any unexpected target rather than filing upstream.

## Sub-issues and dependencies

With gh 2.94 or later, use `gh issue create --parent <parent> ...` or `gh issue edit <parent> --add-sub-issue <child>` for sub-issues. For blocking, use `gh issue create --blocked-by <n>,<n> ...` or `gh issue edit <child> --add-blocked-by <blocker>`.

With older gh versions, use the REST endpoints:

- Sub-issue: `gh api --method POST repos/tmcqueen/openskele/issues/<parent>/sub_issues -F sub_issue_id=<child-db-id>`.
- Dependency: `gh api --method POST repos/tmcqueen/openskele/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`.

Obtain numeric database IDs with `gh api repos/tmcqueen/openskele/issues/<number> --jq .id`; do not substitute issue numbers or node IDs.

If native relationships are unavailable, put `Part of #<parent>` in the child body and add the child to a task list in the parent; record dependencies as `Blocked by: #<n>, #<n>` at the top of the child body. A ticket is unblocked when every blocker is closed.

Count open native blockers with `gh api repos/tmcqueen/openskele/issues/<number> --jq .issue_dependencies_summary.blocked_by`. If using `gh issue view <number> --json blockedBy`, filter out closed blockers.

## Pull requests as a triage surface

**PRs as a request surface: no.**

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Read the GitHub issue using the command above.

## Wayfinding operations

Used by `/wayfinder`. The map is a single issue with child issues as tickets.

- Map: label `wayfinder:map`; body contains Notes, Decisions-so-far, and Fog.
- Child ticket: link it as a sub-issue, using the fallback above when needed. Use a `wayfinder:<type>` label: `research`, `prototype`, `grilling`, or `task`.
- Frontier: consider the map's open children in map order, excluding assigned tickets and tickets with open blockers.
- Claim: `gh issue edit <number> --add-assignee @me`, the session's first write.
- Resolve: comment with the answer, close the ticket, then append a context pointer (gist and link) to the map's Decisions-so-far.
