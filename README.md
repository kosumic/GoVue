# LingoPlay

A port of [LiTube](https://github.com/HydeYYHH/Litube) to **shared React Native UI
and a Rust core**, for Android and iOS. Android is the first implementation and
verification target.

YouTube supplies browsing, comments, subscriptions and account pages inside a
WebView. The app supplies the embedded, fullscreen and mini-player experience,
player menus, queue, extension settings and downloads. Expo Video owns playback,
background audio and picture-in-picture. Rust owns format validation, selection,
settings, queue/history, navigation state, caption processing and bounded media
streaming. Kotlin and Swift adapt platform APIs.

The port follows LiTube commit `bb9ee78428a30347fc6eaf21a679f2a0b8cbc91f`
(2026-10-16). Browser integration scripts are adapted from that source. See
[PORTING.md](PORTING.md) for the acceptance inventory and verification status.
The port is in progress; an implemented control is not evidence of verified
feature parity.

Transcript-reader and AI work is deferred. Their existing modules and stored
data are retained, with their former documentation in
[docs/legacy-features.md](docs/legacy-features.md).

Player subtitles, media resolution and download extraction run through the
shared Rust extractor, independently of the browsing page. An isolated runtime
executes token challenges; platform adapters supply HTTPS bytes and MP4 remuxing.
There is no page-extraction fallback. See [extraction.md](docs/extraction.md) for
the protocol, cache boundaries and current guest-client limitations.

## Development

```sh
nix develop path:.
npm ci
npm run core:android
npx expo prebuild --platform android --no-install
cd android
./gradlew assembleRelease -PreactNativeArchitectures=arm64-v8a
```

Use `npm run page:build` after editing browser injection or decoder source.
`src/youtube/pageBridge.js` is standalone JavaScript for the WebView; the build
generates its injection string, exposed with control helpers by `pageScripts.js`.
Native projects and generated Rust bindings are ignored; persistent native
configuration belongs in `app.json` or a config plugin. iOS builds require an
Apple Silicon Mac with Xcode and `npm run ios` / `npm run prebuild:ios`.

After live Android verification:

```sh
npm run check
npx expo install --check
```

Signed media/caption URLs and raw responses stay in memory. Persisted queues,
history and settings contain only validated, non-secret metadata. The local
native playback transport binds to loopback. Cast uses a separate, revocable
listener on the current private Wi-Fi address under the owner's
[casting-only networking exception](docs/cast-networking.md).

## License

GNU GPL version 3; see [LICENSE](LICENSE). LiTube attribution and dependency
licenses are in [notices/README.md](notices/README.md).
