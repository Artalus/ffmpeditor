# commits

commits should be small, focused, atomic.
if unrelated code needed to be fixed while working on feature - ALWAYS focus these changes as standalone commit.
commit messages explain what logical change was made.
keep messages short; nobody likes to read paragraphs of ai slop. ideally only title without description.

## authoring

whenever performing commit on your own, substitute username & mail for both user and committer to own
and do not use "co-authored by ..." suffix in messages.

# branching

keep `main` history linear.
branch out, 1+ commits, merge with `--no-ff`.
always merge even if only 1 commit.
merge messages explain the intent behind logical changes. omit `merged branch ...`, skip to point

# amending changes

if review requests for changes in the branch, the changes should be committed separately; in multiple commits if logically separated enough.
BEFORE the final merge is done, all the "fix" commits should be squashed into the matching "origin" commits.
if the review called for changes too drastic, consider restructuring commits.

# hooks

`prek` should be used to manage git hooks.
pre-commit hooks focus on formatting.
pre-merge runs tests.
if adding new hook: branch out, commit to add hook, commit to update code to satisfy hook (if needed), merge.

at least use precommit hooks to clippy fmt and clippy fix
consider https://github.com/Artalus/pre-commit-clippy-fix
