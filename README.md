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

The player's Transcript and AI buttons appear before speed and quality. Transcript
opens a searchable reader with language selection, playback following, and
tap-to-seek captions. AI opens the same full-screen conversation workspace as
**Chat** in the bottom navigation. Open the conversation drawer for saved chats,
new chats, providers, and token usage. Opening AI from a video reopens its one
saved conversation, or creates an empty chat with the transcript as context.
Opening chat never sends a message or starts a summary. Choose a model from the
header and send your own question; model changes keep the video's conversation.
Use **Open video** in chat to return to its existing video tab or open it again.
Tap the transcript context card to read or search the captions used by the AI.
Reasoning changes made during a response apply to the next message.
Open **Extensions → AI** to manage AI providers, accounts, models and model
groups.
Paste or write **AGENT.md instructions** there to set your preferred language,
learning level, or response style for all chats and summaries. Save to apply them
to the next message, including in existing chats. Clear and save to remove them.
Choose **Token usage** in AI settings for saved-chat totals, provider/model
breakdowns, cache hit rates and output speed. Open a conversation for its context
and reasoning details. Missing provider counts are shown as unavailable; older
replies may lack cache, timing and completion-date metadata. These are local
saved-history statistics, not the provider's billing or subscription quota.
Choose **ChatGPT / Codex** for account sign-in or **OpenAI API** for an API key.
The alternative **ChatGPT plan sharing** connection is under **Advanced connections**.
Earlier feature documentation remains in
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

Chat supports microphone dictation through `expo-speech-recognition`. Tap the
microphone and speak; partial results appear separately, and only final results
are added to the current draft. Rust owns dictation state and joins recognized
text with spacing appropriate for English, Chinese, and Japanese. Recognition
uses the device locale and up to 100 vocabulary hints built in Rust from the draft,
recent chat messages, and the attached transcript. LingoPlay does not save
audio recordings. Rebuild the native app after installing the dependency so the
speech module and microphone permissions are included. Hint loading falls back
to draft vocabulary after 1.5 seconds. Android finishes on a pause; iOS supports
continuous dictation until stopped.

Signed media/caption URLs and raw responses stay in memory. Persisted queues,
history and settings contain only validated, non-secret metadata. The local
native playback transport binds to loopback. Cast uses a separate, revocable
listener on the current private Wi-Fi address under the owner's
[casting-only networking exception](docs/cast-networking.md).

## License

GNU GPL version 3; see [LICENSE](LICENSE). LiTube attribution and dependency
licenses are in [notices/README.md](notices/README.md).
