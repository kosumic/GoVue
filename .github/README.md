# CI and dependency updates

CI runs on pushes and pull requests targeting `master`, weekly, and manually.
Rust and JavaScript checks run independently. Android generates the Rust bindings
and Expo native project, builds an `arm64-v8a` debug APK, and lints both the app and
the local Expo module. Module lint errors fail the build.
After Android passes, an Apple Silicon macOS runner builds the iOS XCFramework,
installs CocoaPods dependencies, and compiles an unsigned arm64 release app.
`CI complete` requires all four jobs to succeed and can be used as a required
status check. Android APK/lint reports and the unsigned iOS app are retained for
seven days. The unsigned iOS artifact is for compile validation.

Dependabot updates are squash-merged after the entire CI workflow succeeds.
The automation verifies the author, repository, base branch, and tested commit,
then merges with `--match-head-commit`. It skips updates when the base branch has
advanced since testing. This uses the built-in `GITHUB_TOKEN` and does not require
the repository's "Allow auto-merge" setting.

To enable automatic rebase requests after `master` CI recovers, add an Actions
secret named `DEPENDABOT_REBASE_TOKEN`. Use a fine-grained personal access token
restricted to this repository with **Actions: read**, **Contents: read**, and
**Pull requests: read and write** permissions, owned by a user with repository
push access. Enter the token at the prompt; do not put it in shell history:

```sh
gh secret set DEPENDABOT_REBASE_TOKEN --repo kosumic/GoVue
```

Without the secret, rebasing skips with a notice. The workflow only requests a
rebase for failed or timed-out Dependabot PRs that are behind a successfully
tested `master`; repeated requests for the same base and PR commits are ignored.
Both privileged automation workflows avoid checking out or executing PR code.
