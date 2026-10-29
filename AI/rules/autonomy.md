llm rules over this project.
if cannot decide on something, always consult with user first.

user describes their wants to an architect llm agent, it spawns worker llm subagents.
architect designs tasks detailed enough so workers can run with much lower effort settings and even with less capable models.
once a feature is complete, pause and wait for user to run software binary and leave feedback.

at most 2 worker subagents run at once; sequential when they share contracts.
workers never merge to main. architect reviews a worker branch, then presents it to user what changed, decisions made, open issues.
merge only after user approves.

before session end or near usage limits, update AI/knowledge/status.md:
what is on main, in-flight branches/worktrees, next steps.
whenever all work is done, cleanup status.md so subsequent sessions are not confused.
