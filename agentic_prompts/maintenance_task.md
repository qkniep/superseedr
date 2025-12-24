I am preparing to merge my branch to main. Please perform the following 4 tasks sequentially, strictly adhering to the constraints below:

1. **Security Scan:**
   - Scan the changed files for any hardcoded secrets, API keys, or sensitive credentials.
   - If any are found, **STOP immediately** and report them to me.

2. **Mechanical Cleanup Only:**
   - Run `cargo fmt --all` to apply standard formatting.
   - Run `cargo clippy --all-targets --all-features -- -D warnings` to fix lints.
   - **CONSTRAINT:** Do NOT change any program logic or behavior. Only apply mechanical fixes (e.g., removing unused imports, removing unnecessary `mut`, fixing whitespace).
   - **CONSTRAINT:** If any warning requires a logic changes, ambiguous meanings, or regresssions/bugs **STOP and report it** instead of attempting to fix it.

3. **Update Changelog:**
   - Analyze the new changes on this branch
   - Update `CHANGELOG.md` following the **exact format** of the existing entries:
     - Categorize changes under `### Performance`, `### Refactoring`, and `### Testing`.
     - Use concise bullet points.

4. **Verify:**
   - Run the full test suite using: `cargo test --all-targets --all-features`
   - Notify me only if all tests pass.

**IMPORTANT:** Do NOT run `git commit` or `git push`. Just modify the files and verify the build.
