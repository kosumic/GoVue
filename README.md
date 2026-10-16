# LingoPlay

A mobile WebView browser with a YouTube transcript reader, built with **React Native + a shared Rust core** for Android and iOS. It is independent of `../herdr-android`. Litube was reviewed as a reference; no Litube source was copied.

Open a YouTube video and tap **Transcript**. The video remains mounted above the reader. Choose a caption language, search, tap a line to jump and play, or copy/share the transcript. Search shows the number of matches out of all lines and has a clear button. Saved transcripts reopen from the device cache; the refresh button fetches fresh captions.

YouTube videos opened in the browser automatically use the app's player for background audio, notification controls, ad-free video playback, and picture-in-picture, following Litube's browser-to-player handoff. Caption taps seek the active player too. **Web player** explicitly returns to YouTube's HTML player for compatibility. Tap **Download** to choose an MP4 video or M4A audio format; the library supports pause/resume, offline playback, sharing, and removal. Separate video/audio tracks are combined on the device. Playback settings control ad blocking, background playback, automatic PiP, and SponsorBlock skip categories.

The native player follows YouTube's video area while the page stays interactive: descriptions, channel controls, comments, and recommendations remain available. Expanded YouTube panels can cover the video without the native view blocking them. Opening the transcript expands the video above the reader; offline playback uses the full player area.

## Shared architecture

| Layer | Responsibility |
| --- | --- |
| React Native (`App.tsx`, `src/`) | Browser and reader, downloads, shared Expo Video playback, background audio and PiP |
| Page bridges (`src/youtube/`, `scripts/player-decoder.js`) | Capture captions, resolve media using YouTube.js/BgUtils, block ads, control the web player |
| Player runtime (`src/media/PlayerRuntime.tsx`) | Evaluate extracted signature code in a separate renderer with network/file access disabled |
| Rust (`core/`) | Validate responses and formats, parse/search/cache captions, plan downloads, parse SponsorBlock, persist settings/history, serve bounded media ranges |
| Expo module (`modules/lingoplay-core/`) | UniFFI adapters, app-private paths, platform HTTPS requests and MP4 remuxing |

The existing WebView supplies authenticated caption responses and resolves media. Session tokens remain in the page; raw captions cross into Rust for parsing and are never persisted. Signature solving runs outside YouTube's page, following Litube's isolation approach. Signed media URLs cross into native memory for playback/downloads, but are never saved to history or logged. A Rust loopback server adapts bounded YouTube byte requests to the HTTP ranges native players and downloaders expect, including a scoped GET-to-POST compatibility retry; only loopback allows plain HTTP. Native calls run outside the JavaScript UI thread. Generated bindings, Android libraries, and the iOS XCFramework are build outputs. Upstream license notices are in `notices/`.

## Development

```sh
nix develop path:.
npm ci
npm run android
```

Connect an arm64 Android device with USB debugging enabled. Expo generates the native Android project and starts Metro; Gradle builds the Rust library and bindings automatically. The Nix environment supplies Node, Rust 1.90, cargo-ndk, the Android Rust target, SDK/NDK, and Java.

For USB networking:

```sh
adb reverse tcp:8081 tcp:8081
npm start
adb shell am start -a android.intent.action.VIEW \
  -d 'lingoplay://expo-development-client/?url=http%3A%2F%2F127.0.0.1%3A8081' \
  io.github.kaminarios.lingoplay
```

On an Apple Silicon Mac with Xcode:

```sh
nix develop path:.
npm ci
npm run ios
```

This builds Rust for both the iOS device and simulator targets, generates Swift bindings and an XCFramework, then runs Expo. Use `npm run prebuild:ios` when only regenerating the native project. The Darwin shell selects Xcode's tools and clears Nix host compiler/SDK overrides that interfere with iOS linking. A development build is required; Expo Go cannot load the local Rust module.

Both platforms share the reader, extraction, playback controls, and download workflow. Small Kotlin and Swift adapters use their platform HTTPS and MP4 APIs. Root `android/` and `ios/` directories are generated and ignored; persistent native module sources live under `modules/`. `npm ci` bundles the page decoder; run `npm run page:build` after changing its source.

On Android, `npm ci` also applies the Expo Video 57 PiP activity fix in `scripts/patch-video.mjs`. Autolinking builds `expo-video` from source so that fix is included instead of using its precompiled binary.

## Verification and builds

After the feature works live:

```sh
npm run check
npx expo install --check
```

These check TypeScript, exercise the exact injected JavaScript with realistic browser fixtures, and test the Rust boundary, caption formats, search/export, and disk restore. They do not replace YouTube/device checks.

Standalone Android APK, inside the Nix shell:

```sh
npx expo prebuild --platform android --no-install
NODE_ENV=production android/gradlew -p android :app:assembleRelease \
  -PreactNativeArchitectures=arm64-v8a
adb install -r android/app/build/outputs/apk/release/app-release.apk
```

The release variant bundles JavaScript and works without Metro. It currently uses the development signing key; production signing/distribution are not configured.

WebView debugging is disabled by default. Set `EXPO_PUBLIC_LINGOPLAY_DEBUG=1` only for a temporary diagnostic build.

The app has been built, installed, and exercised on the Pixel: extraction, search, copy, caption seeking, inline native playback with scrollable and expanded YouTube comments, downloading separate video/audio tracks into a combined MP4, pause/resume, offline playback, background playback, and PiP. SponsorBlock skipped an actual community segment; a known ad request was intercepted with page rules enabled. The iOS Release app has been built and launched on an iPhone 17 Pro simulator on `mini`; the Rust library also builds for arm64 iOS devices. YouTube behavior on iOS remains unverified because YouTube connections time out on that host. No physical iPhone or distribution signing has been verified.

## Extraction and cache

The page bridge asks the existing YouTube player for a caption track and captures the player's fetch or XMLHttpRequest response, including requests requiring a proof-of-origin token. It clears and reselects an uncached track so a paused player fetches its text, then restores the previous selection. Default extraction and fallback requests honor the selected track, YouTube's default caption track, and the page language. Where available, it can also use YouTube's transcript control, transcript endpoint, or caption-track URL. There is no external transcript service or audio upload.

The Rust boundary accepts Android's origin-only WebMessageListener events and document-URL events from other WebView bridges, while validating the active request and video identity. Navigation cancels pending extraction and clears the reader, including single-page navigation. Raw JSON3/XML responses are parsed in Rust; transcript-renderer fallback segments receive the same validation and normalization.

Normalized transcripts are cached in app-private storage (up to 20 documents / 24 MB, with three documents in memory). Raw caption responses, signed URLs, and session tokens are not persisted. Cache write failures leave the current transcript usable. Corrupt entries are ignored on restore. Refresh bypasses the saved document.

The WebView keeps its initial source and full page height when the transcript opens; its visible region is clipped so YouTube does not mistake the smaller window for picture-in-picture and pause playback. Caption taps use the existing player's seek and play controls.

YouTube's internal endpoints can change. Videos without captions, restricted videos, unavailable live captions, and token-protected responses may not yield a transcript. Retry after playing or reloading the video. This app reads available captions; it does not transcribe audio or translate text.

Downloads support finite AVC/AAC MP4 streams and M4A audio. Native online playback currently requires a combined video/audio stream. Live-stream downloads are unavailable. Pause/resume works while the app process survives; an interrupted process requires opening the video and retrying. Saved files and download history persist across restarts. Ad blocking hides known placements, blocks known ad requests, and skips detected video ads; YouTube changes can affect its effectiveness. SponsorBlock uses community data and continues playback if the service is unavailable.
