# docs

AI/docs is the source of truth for what the program does.
a change starts as an edit to docs stating the new truth; code follows; review checks code against docs.
a branch is not done until docs match the code.
docs changes may be committed together with the matching code changes.

`architecture.md`: how it works. for llm and user.
`user-guide.md`: how to use it. for the user.

a design covering several branches lives on a docs-only reference branch.
that branch is not merged; main docs never describe unbuilt features.
review checks code against that docs commit; mismatches are fixed in docs or code before presenting.

user guide is extremely terse. menu paths, keys, plain sentences.
no filler, no marketing words, no "simply"/"seamlessly"/"robust", no summaries of what was just said,
no dash-chained asides, no bold for emphasis, no emoji.
free of typical llm junkwords everyone is tired of.
