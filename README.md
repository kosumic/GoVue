# LingoPlay

A mobile WebView browser with a YouTube transcript reader, built with **React Native + a shared Rust core** for Android and iOS. It is independent of `../herdr-android`. Litube was reviewed as a reference; no Litube source was copied.

Open a YouTube video and tap **Transcript**. The video remains mounted above the reader. Choose a caption language, search, tap a line to jump and play, or copy/share the transcript. Search shows the number of matches out of all lines and has a clear button. Saved transcripts reopen from the device cache; the refresh button fetches fresh captions.

## Shared architecture

| Layer | Responsibility |
| --- | --- |
| React Native (`App.tsx`, `src/components/`) | Browser controls, transcript reader, clipboard/share, playback actions |
| Page bridge (`src/youtube/pageBridge.js`) | Access the visible YouTube session, capture caption responses, control the existing player |
| Rust (`core/`) | Validate origin/request/video identity, parse JSON3 and XML captions, normalize text, search, export, persist transcripts |
| Expo module (`modules/lingoplay-core/`) | Small Kotlin/Swift adapters to the same generated UniFFI function; provide the native sandbox path |

Rust does not fetch YouTube content separately: the existing WebView supplies its authenticated caption response. Session tokens and signed URLs stay inside the page. Native calls run asynchronously outside the JavaScript UI thread. Generated bindings, Android libraries, and the iOS XCFramework are build outputs.

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

Both platforms share the reader and extraction code. The Kotlin and Swift adapters only expose the Rust function and choose an app-private storage directory. Root `android/` and `ios/` directories are generated and ignored; persistent native module sources live under `modules/`.

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

The Rust-backed app has been built, installed, and exercised on the Pixel: extraction, search, copy, and caption seeking. The iOS Release app has been built and launched on an iPhone 17 Pro simulator on `mini`; the Rust library also builds for arm64 iOS devices. YouTube extraction on iOS remains unverified because YouTube connections time out on that host. No physical iPhone or distribution signing has been verified.

## Extraction and cache

The page bridge asks the existing YouTube player for a caption track and captures the player's fetch or XMLHttpRequest response, including requests requiring a proof-of-origin token. It restores the previous player caption selection afterward. Where available, it can also use YouTube's transcript control, transcript endpoint, or caption-track URL. There is no external transcript service or audio upload.

The Rust boundary accepts Android's origin-only WebMessageListener events and document-URL events from other WebView bridges, while validating the active request and video identity. Navigation cancels pending extraction and clears the reader, including single-page navigation. Raw JSON3/XML responses are parsed in Rust; transcript-renderer fallback segments receive the same validation and normalization.

Normalized transcripts are cached in app-private storage (up to 20 documents / 24 MB, with three documents in memory). Raw caption responses, signed URLs, and session tokens are not persisted. Cache write failures leave the current transcript usable. Corrupt entries are ignored on restore. Refresh bypasses the saved document.

The WebView keeps its initial source and full page height when the transcript opens; its visible region is clipped so YouTube does not mistake the smaller window for picture-in-picture and pause playback. Caption taps use the existing player's seek and play controls.

YouTube's internal endpoints can change. Videos without captions, restricted videos, unavailable live captions, and token-protected responses may not yield a transcript. Retry after playing or reloading the video. This app reads available captions; it does not transcribe audio or translate text.
