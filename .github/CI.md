# CI and dependency updates

CI runs on pushes and pull requests targeting `master`, weekly, and manually.
Rust and JavaScript checks run independently. Android generates the Rust bindings
and Expo native project, builds an `arm64-v8a` debug APK, and lints both the app and
the local Expo module. Module lint errors fail the build. JavaScript checks include
ESLint's recommended JavaScript and TypeScript rules and React's rules of hooks.
`npm run lint` rejects warnings and unused ESLint disable directives; it also runs
as part of `npm run check`. Generated and vendored sources are excluded from lint.
The Android job removes the hosted runner's preinstalled Android SDK and unused
language runtimes before installing the Nix toolchain to leave room for native
builds. Disk usage is logged before and after cleanup.
After Android passes, an Apple Silicon macOS runner builds the iOS XCFramework,
installs CocoaPods dependencies, and compiles an unsigned arm64 release app.
`CI complete` requires every applicable job to succeed and can be used as a
required status check. Android APK/lint reports and the unsigned iOS app are
retained for seven days. The unsigned iOS artifact is for compile validation.

Like Whip, CI runs CodeQL for JavaScript/TypeScript in a separate job and for
Java/Kotlin around the Android build. Gradle's build cache is disabled so CodeQL
observes compilation of the generated Expo project and local native module.
Both analyses upload results to GitHub code scanning.

On `master`, the Android build also resolves `:app`'s `releaseRuntimeClasspath`
and generates a dependency graph containing the app's transitive native runtime
dependencies. An empty or missing snapshot fails CI. A separate job submits the
snapshot with `contents: write` without checking out or executing app code; the
Android build itself retains `contents: read`. Snapshot artifacts expire after
one day. Pull requests run dependency review and reject newly introduced
vulnerabilities rated moderate or higher in GitHub's dependency graph. The native
snapshot is submitted on `master`; PR review does not generate a native snapshot
for the proposed Gradle changes.

Enable **Dependency graph** in the repository's
[Advanced Security settings](https://github.com/kosumic/GoVue/settings/security_analysis)
before the first run. Dependency review and snapshot submission require it.

CodeQL, dependency review, and dependency submission are included in the
`CI complete` gate, so Dependabot waits for applicable checks before merging.
Only dependency review outside PRs and dependency submission outside `master`
may be skipped successfully.

The app version lives in `package.json`. Like Whip, `app.config.js` reads it
through `scripts/app-version.cjs` so Expo generates both Android and iOS with
the same version. `npm run check:app-version` validates the resolved Expo
configuration and rejects version overrides in `app.json`; it runs in CI,
`npm run check`, and release builds. Use `npm version patch --no-git-tag-version`
(or `minor`, `major`, or an explicit version) to update the package and lockfile.

The manual `Build Android APK` workflow follows Whip's release build workflow.
Run it from Actions with a tag matching `package.json`, for example `v0.1.0`.
The shared version helper rejects mismatched tags. The workflow generates the
native Android project, builds a standalone `arm64-v8a` release APK with embedded
JavaScript, and uploads
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
