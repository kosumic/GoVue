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

The Android browser uses `androidx.webkit:webkit:1.14.0`. Register
`io.github.kaminarios.govue.shell.BrowserTab` on `NshView` before starting the
Rust app. Give each retained tab a `BrowserTab` with its initial URL and last
document generation. Feed its view the current media options and keep it
mounted while a GoVue screen covers it.

`BrowserEvent::Navigation` supplies the URL, generation, and history state.
`PageMessage` supplies the native origin and document. Check the owning tab,
active video, and pending request before applying messages to Rust state.
`BrowserTab::inject` targets a single document generation. The app handles
external links and errors using the corresponding browser events.

Create one `CaptionPage` beside `Core` in the app root and pass it to each tab
view. Select the active tab with `CaptionPage::select`. The adapter delivers
Rust extraction requests through GoVue's caption script after the document is
ready. It releases that document's registration when the active tab changes.

Register `CaptionOverlay` with `NshView` and mount `caption_overlay` over the
player surface. Use `player_caption_frame` for the displayed cue and
`media_settings.subtitleStyle` for its appearance. Pass the overlay's layout
event into the next `player_caption_frame` request so the Rust player can page
long cues at the correct time.

Register `io.github.kaminarios.govue.shell.PlayerRuntime` with the Android
`NshView` before calling `runRustApp`. Construct the Rust `PlayerRuntime` in the
player component and mount its hidden `view()`. Pass the pending media request
ID to `evaluate`, then cancel that request when its video or document changes.
Rust manages the queue, timeouts, cancellation, and request IDs. Each
evaluation gets a fresh local WebView and returns the `n` and `sig` strings.

From GoVue’s Nix Android environment, check the runtime crate.

```sh
npm run page:build
cargo clippy --locked --manifest-path native-shell/runtime/Cargo.toml \
  --target aarch64-linux-android -- -D warnings
```
