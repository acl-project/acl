Do not make any modifications to the code, except for unit tests.

# Unit Test Writing Rules

- **Describe your plan first.** Before writing any unit tests, explain what tests you intend to add, which behaviors they will cover, and which files you plan to modify.
- **Obtain explicit approval.** Do not write or modify any unit tests until explicit approval has been given.
- **Strictly limit the scope of changes.** You are only allowed to write or modify unit tests. All other code modifications are strictly prohibited.
- Do not modify production code, configuration files, build scripts, dependencies, or other project files.
- If the required unit tests cannot be written without modifying files outside the permitted scope, explain the limitation and do not modify other code.

# Code Review Guidelines

When reviewing the code, follow these guidelines:

- Check for potential variable naming issues and spelling mistakes in log messages.
- Ensure that log messages start with a capital letter.
- Check for inappropriate or inconsistent naming.
- Identify unnecessary or unused code.
- Check whether important comments are missing.
- Check whether unit tests adequately cover new features.
- Verify that the formatting, linting, and tests pass across the entire Cargo workspace.
- Ensure that all comments and log messages are written in English.
- Verify that changes are being made on a branch other than `main`.
- Ensure that branch names follow the `feat/` or `fix/` naming conventions, as appropriate.
- Strictly forbidden to cause crashes due to `unwrap`.

# Mandatory Pre-Delivery Verification

Before delivering any changes, you must run the following commands from the workspace root and verify that all of them pass for the entire workspace:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

**All three checks must pass across the entire workspace before the work can be considered complete.**

- Do not assume that a check passes simply because the code appears correct.
- Do not claim that any check passed unless you actually ran the corresponding command and verified its result.
- If any check fails, investigate the failure. You may fix issues only within the permitted unit-test scope. Do not modify production code or other prohibited files to make the checks pass.
- If a failure cannot be resolved within the permitted scope, report the failing command and the relevant error clearly. Do not claim that the work is fully verified.
- If the commands cannot be executed, explicitly report which checks remain unverified. Never claim successful verification without evidence.

# Important Restrictions

Code review must remain read-only. Do not fix issues or modify existing code during a review.

Any proposed unit test changes must follow the Unit Test Writing Rules above and receive explicit approval before implementation.

The restriction on code modifications applies throughout the entire workflow, including any attempts to resolve formatting, linting, or test failures. Passing the checks does not authorize modifications outside the permitted scope.