llm rules over this project.
if cannot decide on something, always consult with user first.

# architect <-> worker

user describes their wants to an architect llm agent, it spawns worker llm subagents.
architect designs tasks detailed enough so workers can run with much lower effort settings and even with less capable models.
at most 2 worker subagents run at once; sequential when they share contracts.

workers default to sonnet, effort medium; stronger model only for design-heavy tasks.
may use lower model/effort if task is deemed simple/detailed enough for it.

worker brief:
* identity (so it does not consider itself an architect)
* exact goal, file:line anchors, at most 8 items, spec = docs commit sha.
* hygiene: grep and ranged reads, batched edits, test once per commit, never commands reading stdin.
> goal is to offhand work to sessions 1) with free context to avoid hallucinating, 2) that won't need to dig and reiterate, and 3) thus should complete fast.

architect may make tiny fixes (about 10 lines) itself instead of spawning a worker.

## review

once a feature is complete, pause and wait for user to run software binary and leave feedback.
before presenting UI changes, architect verifies them visually (make a screenshot recipe and store in AI/knowledge/tools).
> test on a **copy** of the user's test project; never modify the user's project files.
temporary debug code is never committed; revert it before presenting.
stack a branch on another unmerged branch only when user asks.

workers never merge to main. architect reviews a worker branch, then presents it to user what changed, decisions made, open issues.
merge only after user approves.

# persistence

before session end or near usage limits, update AI/knowledge/status.md:
what is on main, in-flight branches/worktrees, next steps.
whenever all work is done, cleanup status.md so subsequent sessions are not confused.
