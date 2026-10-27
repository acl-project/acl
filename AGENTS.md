Do not make any modifications to the code, except for unit tests.

# Unit Test Writing Rules

## 1. Planning & Approval
- **Describe your plan first:** Before writing any tests, explain what you intend to add, which behaviors they cover, and which files you will modify.
- **Obtain explicit approval:** Do not write or modify any tests until you get my explicit approval.

## 2. Scope & Constraints
- **Test-only modifications:** You are strictly limited to writing or modifying unit tests. 
- **No side effects:** Do not modify production code, configuration files, build scripts, dependencies, or any other project files.
- **Explain limitations:** If writing the required tests requires modifying files outside this scope, explain the limitation and stop. Do not modify other code.

## 3. Testing Principles
- **Hermetic & Deterministic:** Unit tests must be strictly isolated. Do not depend on the local environment, external asset files, network, or file systems. Mock external dependencies and use in-memory data instead.
- **Follow expected behavior:** Write tests based on the expected behavior of requirements, not the existing code (no reverse-fitting). If the behavior is unclear, ask me directly—do not guess or make assumptions.
- **No cheating to pass:** When a test fails, first determine whether the test code is incorrect or the production code has a bug. 
  - If the test code is wrong, fix the test code.
  - If the production code has a bug, point out the issue explicitly, but **do not modify the production code**.
- **No masking bugs:** It is strictly forbidden to delete test cases, disable tests, or alter test logic to cover up real bugs.
  
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
- Production code must not contain code that can cause crashes, such as unwrap/expect/panic!, direct slice indexing, raw arithmetic, etc.

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