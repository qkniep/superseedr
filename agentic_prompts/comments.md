I am preparing to merge my branch to main. Analyze all new comments in this branch.

Do not commit your new changes - also only review NEW comments, do not touch older comments.

1. REMOVE: Comments that merely describe "what" the code is doing (e.g., "// increments i by 1").
2. REMOVE: Commented-out code blocks (dead code).
3. REMOVE: Misleading and incorrect comments.
4. KEEP/SIMPLIFY: Comments that explain "why" a specific, non-obvious approach was taken (business logic, edge cases, specific bug fixes).
5. CONSOLIDATE: Merge adjacent single-line comments into concise multi-line blocks or single summaries where applicable.
6. REFACTOR: If comment can be replaced by renaming, this is a better solution.
