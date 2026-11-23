# GoVue Native Shell app

The shared Rust UI lives in `app/`. The `runtime/` crate connects it to
`govue-core` and the Android and iOS hosts.

Build the standalone Android app with GoVue's Nix Android toolchain and Gradle
9.4.1. Cargo supplies the Android host Java sources from the locked Native Shell
package. Set `NSH_SOURCE` to build against a compatible local checkout, or
`GRADLE` to use a specific Gradle executable.

```sh
npm --prefix native-shell ci
nix develop path:. --command bash native-shell/android/build-release.sh
```

The ARM64 release APK is in `native-shell/android/build/outputs/apk/release`.
The default application ID is `io.github.kaminarios.govue`. Supply the original
Android distribution signing key through `GOVUE_ANDROID_KEYSTORE`,
`GOVUE_ANDROID_STORE_PASSWORD`, `GOVUE_ANDROID_KEY_ALIAS`, and
`GOVUE_ANDROID_KEY_PASSWORD` for an in-place upgrade. A build without a key
produces an unsigned APK. Set `GOVUE_PREVIEW=1` for a debug-signed release APK
with the separate `io.github.kaminarios.govue.nsh` identity.

On Apple Silicon with Xcode, build the iPhone or iPad simulator release with
the same standalone page-bundler dependencies.

```sh
npm --prefix native-shell ci
GOVUE_PREVIEW=1 bash native-shell/ios/build-release.sh simulator
```

The simulator bundle is at
`native-shell/ios/build/iphonesimulator/GoVue.app`. The iOS host uses the
shared Rust browser, player controls, captions, and Chat screens. The retained
WKWebView supplies the watch-page layout, and Native Shell's AVPlayer plays the
HLS source planned by GoVue's Rust core.
`Core::ios()` resolves the original `Application Support/LingoPlay/transcripts`
directory within the app sandbox.
Use `bash native-shell/ios/build-release.sh device` for the unsigned device
bundle under `native-shell/ios/build/iphoneos/GoVue.app`. Sign it with the
existing GoVue team and provisioning profile for device upgrades.

On Android, construct `Core::android()` in the Native Shell app root. It uses
GoVue’s existing app-private `files/transcripts` directory. Android retains that
sandbox when the APK uses the existing app ID and signing key.
`Core::execute` and `Core::ai` run on Native Shell workers. Subscribe before
starting an operation that publishes updates, then reload the affected state
from Rust when its channel changes.

The Android browser uses AndroidX WebKit. Register
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
