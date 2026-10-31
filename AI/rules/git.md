# commits

commits should be small, focused, atomic.
if unrelated code needed to be fixed while working on feature - ALWAYS focus these changes as standalone commit.
commit messages explain what logical change was made.
keep messages short; nobody likes to read paragraphs of ai slop. ideally only title without description.
every commit must pass hooks (fmt, clippy -D warnings) and tests on its own.
a function lands in the same commit as its first caller.

## authoring

whenever performing commit on your own, substitute username & mail for both user and committer to own
and do not use "co-authored by ..." suffix in messages.
own identity = `Claude <model> <noreply@anthropic.com>`, e.g. `Claude Sonnet 5.5`.

# branching

keep `main` history linear.
branch out, 1+ commits, merge with `--no-ff`.
always merge even if only 1 commit.
merge messages explain the intent behind logical changes. omit `merged branch ...`, skip to point

keep branches focused on single goal:
* `shrink and dedupe the codebase; fix trim crash, silent config overwrite, orphaned preview ffmpeg and leftover partial exports` - bad message, too many things
* `timeline widget to arrange, move and trim clips with the mouse` - better, more focused

refactors get their own branch, merged before the feature that needs it.
prep work happens in separate branch if heavy enough, otherwise same branch.
one branch per worktree under .claude/worktrees; user removes worktrees.

# amending changes

if review requests for changes in the branch, the changes should be committed separately; in multiple commits if logically separated enough.
BEFORE the final merge is done, all the "fix" commits should be squashed into the matching "origin" commits.
if the review called for changes too drastic, consider restructuring commits.
before presenting a branch, dry-run the autosquash on a copy; it must apply cleanly.
squash fixups at merge with `--committer-date-is-author-date`; the final tree must equal the tested tip.
if autosquash conflicts, rebuild commits from snapshot trees instead of resolving step by step (check AI/knowledge/).

# hooks

`prek` should be used to manage git hooks.
pre-commit hooks focus on formatting.
pre-merge runs tests.
if adding new hook: branch out, commit to add hook, commit to update code to satisfy hook (if needed), merge.

at least use precommit hooks to clippy fmt and clippy fix
consider https://github.com/Artalus/pre-commit-clippy-fix
