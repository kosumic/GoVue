# CI and dependency updates

CI runs on pushes and pull requests targeting `master`, weekly, and manually.
Rust and JavaScript checks run independently. Android builds and lints the
standalone Native Shell `arm64-v8a` release APK with the preview app ID and test
signing key.
JavaScript checks cover the shared browser bridges and game renderer fixtures.
`npm run lint` rejects warnings and unused ESLint disable directives; it also runs
as part of `npm run check`. Generated and vendored sources are excluded from lint.
The Android job removes the hosted runner's preinstalled Android SDK and unused
language runtimes before installing the Nix toolchain to leave room for native
builds. Disk usage is logged before and after cleanup.
After Android passes, an Apple Silicon macOS runner compiles an unsigned arm64
Native Shell iOS app.
`CI complete` requires every applicable job to succeed and can be used as a
required status check. Android APK/lint reports and the unsigned iOS app are
retained for seven days. The unsigned iOS artifact is for compile validation.

JWT verification uses `jsonwebtoken` with its AWS-LC backend. The RustCrypto
backend pulls in `rsa`, which has no patched release for RUSTSEC-2023-0071 and
fails the Rust dependency audit.

Like Whip, CI runs CodeQL for JavaScript/TypeScript in a separate job and for
Java/Kotlin around the Android build. Gradle's build cache is disabled so CodeQL
observes compilation of the Android host.
Both analyses upload results to GitHub code scanning.

Release builds install the page bundler from `native-shell/package-lock.json`.
The root lockfile adds the browser test and lint tools used by CI.

On `master`, the Android build also resolves the root project's `releaseRuntimeClasspath`
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

The app version lives in `package.json`. Both hosts read that version during the
build, and the release workflow validates the Android package and iOS bundle
metadata against it. Use `npm version patch --no-git-tag-version`
(or `minor`, `major`, or an explicit version) to update the package and lockfile.

The manual `Build Android APK` workflow follows Whip's release build workflow.
Run it from Actions with a tag matching `package.json`, for example `v0.1.0`.
The shared version helper rejects mismatched tags. The workflow builds and signs
a standalone `arm64-v8a` release APK with GoVue's original application ID and
uploads
`govue-arm64.apk` and its SHA-256 checksum as a `govue-arm64-apk` artifact. It also
creates or updates the GitHub Release for that tag and replaces its APK/checksum
assets. New tags point to the workflow's selected commit.

After Android succeeds, the workflow compiles an unsigned arm64 iOS release app
on Apple Silicon. The
`govue-ios-unsigned-compile-only` artifact contains the app ZIP for compile
validation; it requires signing before installation on a device. The manual
workflow retains its artifacts for 14 days, while ordinary CI retains them for
seven days.

Configure `GOVUE_ANDROID_KEYSTORE_BASE64`, `GOVUE_ANDROID_STORE_PASSWORD`,
`GOVUE_ANDROID_KEY_ALIAS`, and `GOVUE_ANDROID_KEY_PASSWORD` as Actions secrets
before running the release workflow. An in-place upgrade requires the same
distribution signing key as the installed APK.

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
