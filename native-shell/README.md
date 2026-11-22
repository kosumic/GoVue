# GoVue Native Shell port

The runtime crate uses Native Shell 0.6.1 and the existing `govue-core` crate.
Use the current GoVue app as the reference for screen layout, navigation,
gestures, and feature behavior. Preserve the YouTube launch screen, Chat in
the bottom navigation and video player, and the existing player controls
for subtitles and the transcript reader.

Build the standalone Android app with GoVue's Nix Android toolchain, Gradle
9.4.1, and the Native Shell v0.6.1 sources at `../nsh`. Set `NSH_SOURCE`
for a different checkout location and `GRADLE` for a different Gradle
executable. Install the standalone page-bundler dependencies before building.

```sh
npm --prefix native-shell ci
nix develop path:. --command bash native-shell/android/build-release.sh
```

The ARM64 release APK is in `native-shell/android/build/outputs/apk/release`.
During migration its application ID is `io.github.kaminarios.govue.nsh`.

On Apple Silicon with Xcode, build the iPhone or iPad simulator release with
the same standalone page-bundler dependencies.

```sh
npm --prefix native-shell ci
bash native-shell/ios/build-release.sh simulator
```

The simulator bundle is at
`native-shell/ios/build/iphonesimulator/GoVue.app`. The iOS host uses the
shared Rust browser, player controls, captions, and Chat screens. The retained
WKWebView supplies the watch-page layout, and Native Shell's AVPlayer plays the
HLS source planned by GoVue's Rust core.
`Core::ios()` resolves the original `Application Support/LingoPlay/transcripts`
directory within the app sandbox.

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

The Android video surface uses Media3 `media3-exoplayer`,
`media3-exoplayer-dash`, and `media3-exoplayer-hls` 1.9.1. Register
`io.github.kaminarios.govue.shell.VideoPlayer` with `NshView`. Set
`android:networkSecurityConfig="@xml/govue_network_security"` in the app
manifest so the Rust media proxy can use loopback HTTP.

Keep the Rust `VideoPlayer` view mounted over the page player bounds. Pass
watch-tab messages through `PageVideoLayout::receive` and feed their scaled
frame to `VideoPlayer::embedded_view`. That view follows page movement and
visibility changes. Prepare a source with
`PlaybackSource::prepare_android` and send playback commands with the
generation returned by `VideoPlayer::replace`. Use status positions for
captions and player controls. The player releases its previous loopback
routes after the native replacement is applied.

On Android, `WatchPlayback` connects the retained browser and player. Give
it the watch tab, persisted settings, and foreground state. Pass the tab's
browser events to `receive` and mount `view` in the browser's workspace.
The controller probes the page position before taking over, scopes
extraction to the active document, and keeps the source when navigating
within the same video.

`WatchSubtitles` uses the watch video, saved subtitle track and style, and
the native playback position. Mount its view above the video surface.
Switching tracks cancels the previous extraction and clears its cue.

`TranscriptReader` keeps the existing Rust reader session alive while a
panel is closed, and releases it when the active video changes. It exposes
the same cached data, track comparison, search, and export operations.

Register `io.github.kaminarios.govue.shell.PlayerRuntime` with the Android
`NshView` before calling `runRustApp`. Construct the Rust `PlayerRuntime` in the
player component and mount its hidden `view()`. Pass the pending media request
ID to `evaluate`, then cancel that request when its video or document changes.
Rust manages the queue, timeouts, cancellation, and request IDs. Each
evaluation gets a fresh local WebView and returns the `n` and `sig` strings.

Register `io.github.kaminarios.govue.shell.TokenRuntime` with the same
host. Mount one Rust `TokenRuntime` beside `Core` to handle the shared
extractor's verification tasks. Each request retains its own local WebView
from snapshot through mint. The interpreter runs without network access,
cookies, or a native JavaScript interface.

From GoVue’s Nix Android environment, check the runtime crate.

```sh
npm run page:build
cargo clippy --locked --manifest-path native-shell/runtime/Cargo.toml \
  --target aarch64-linux-android -- -D warnings
```
