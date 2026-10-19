# CI and dependency updates

CI runs on pushes and pull requests targeting `master`, weekly, and manually.
Rust and JavaScript checks run independently. Android generates the Rust bindings
and Expo native project, builds an `arm64-v8a` debug APK, and lints both the app and
the local Expo module. Module lint errors fail the build.
The Android job removes the hosted runner's preinstalled Android SDK and unused
language runtimes before installing the Nix toolchain to leave room for native
builds. Disk usage is logged before and after cleanup.
After Android passes, an Apple Silicon macOS runner builds the iOS XCFramework,
installs CocoaPods dependencies, and compiles an unsigned arm64 release app.
`CI complete` requires all four jobs to succeed and can be used as a required
status check. Android APK/lint reports and the unsigned iOS app are retained for
seven days. The unsigned iOS artifact is for compile validation.

The manual `Build Android APK` workflow follows Whip's release build workflow.
Run it from Actions with a tag matching the versions in `package.json` and
`app.json`, for example `v0.1.0`. It generates the native Android project, builds a
standalone `arm64-v8a` release APK with embedded JavaScript, and uploads
`govue-arm64.apk` and its SHA-256 checksum as a `govue-arm64-apk` artifact. It also
creates or updates the GitHub Release for that tag and replaces its APK/checksum
assets. New tags point to the workflow's selected commit.

After Android succeeds, the workflow generates the Rust XCFramework and iOS
project on Apple Silicon, installs CocoaPods dependencies outside Nix, and
compiles an unsigned arm64 release app with embedded JavaScript. The
`govue-ios-unsigned-compile-only` artifact contains the app ZIP for compile
validation; it requires signing before installation on a device. The manual
workflow retains its artifacts for 14 days, while ordinary CI retains them for
seven days.

The release APK currently uses Expo's development signing key, matching local
release builds. Production Android signing is not configured. This workflow
requires no signing secrets or EAS account, and the iOS app is only uploaded as
an Actions artifact.

Dependabot updates are squash-merged after the entire CI workflow succeeds.
TypeScript stays within Expo SDK 57's supported `~6.0.3` range; update its
Dependabot bound when upgrading Expo.
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
