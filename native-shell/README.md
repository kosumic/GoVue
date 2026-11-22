# GoVue Native Shell port

The runtime crate uses Native Shell 0.6.1 and the existing `govue-core` crate.
Use the current GoVue app as the reference for screen layout, navigation,
gestures, and feature behavior. Preserve the YouTube launch screen, Chat in
the bottom navigation and video player, and the existing player controls
for subtitles and the transcript reader.

On Android, construct `Core::android()` in the Native Shell app root. It uses
GoVue’s existing app-private `files/transcripts` directory. Reuse the app ID and
signing identity in the eventual upgrade so Android retains that sandbox.
`Core::execute` and `Core::ai` run on Native Shell workers. Subscribe before
starting an operation that publishes updates, then reload the affected state
from Rust when its channel changes.

Register `io.github.kaminarios.govue.shell.PlayerRuntime` with the Android
`NshView` before calling `runRustApp`. Construct the Rust `PlayerRuntime` in the
player component and mount its hidden `view()`. Pass the pending media request
ID to `evaluate`, then cancel that request when its video or document changes.
Rust manages the queue, timeouts, cancellation, and request IDs. Each
evaluation gets a fresh local WebView and returns the `n` and `sig` strings.

From GoVue’s Nix Android environment, check the runtime crate.

```sh
cargo clippy --locked --manifest-path native-shell/runtime/Cargo.toml \
  --target aarch64-linux-android -- -D warnings
```
