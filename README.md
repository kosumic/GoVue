# LingoPlay

A mobile WebView browser with a YouTube transcript reader, built with **React Native + a shared Rust core** for Android and iOS. It is independent of `../herdr-android`. Litube was reviewed as a reference; no Litube source was copied.

Open a YouTube video and tap **Transcript**. The video remains mounted above the reader. The current caption has a green highlight and side marker, and the reader follows playback in either player. Scroll to read ahead, then tap **Follow playback** to resume following. Choose a caption language, search, tap a line to jump and play, or copy/share the transcript. Search shows the number of matches out of all lines and has a clear button; the current line stays highlighted in matching results without moving the search list. Saved transcripts reopen from the device cache; the refresh button fetches fresh captions.

YouTube videos opened in the browser automatically use the app's player for background audio, notification controls, ad-free video playback, and picture-in-picture, following Litube's browser-to-player handoff. Caption taps seek the active player too. **Web player** explicitly returns to YouTube's HTML player for compatibility. Tap **Download** to choose an MP4 video or M4A audio format; the library supports pause/resume, offline playback, sharing, and removal. Separate video/audio tracks are combined on the device. Playback settings control ad blocking, background playback, automatic PiP, and SponsorBlock skip categories.

The native player follows YouTube's video area while the page stays interactive: descriptions, channel controls, comments, and recommendations remain available. Expanded YouTube panels can cover the video without the native view blocking them. Opening the transcript expands the video above the reader; offline playback uses the full player area.

Tap **Minimize browser** beside the playback controls to keep listening while using LingoPlay. An open transcript fills the available space. The page and player stay mounted, including when you open the start page. Tap **Return to browser** to restore the same page and playback position; the bar also offers play/pause and remains available in chat, saved words, downloads, and settings.

The **Speed** and **Quality** buttons expose playback speed (0.25×–3×) and the video's available resolutions. **Auto** adapts between available segmented tracks. Separate AVC video and AAC audio play through in-memory HLS playlists on the Rust loopback transport; no download or remux is needed. Changing resolution preserves position, speed, and whether playback is paused. Speed and resolution preferences are saved by Rust; offline files support speed changes and keep their downloaded resolution.

Open **Settings** (the gear button) to add **Languages I know** with current levels and **Languages I want to learn** with target levels. Pick a language from 196 bundled choices: common languages appear first, followed by an alphabetical list, with optional search by English name, native name, alias, or code. The list covers [ISO 639-1 languages](https://www.loc.gov/standards/iso639-2/php/code_list.php) plus additional languages including Cantonese and Filipino. Languages already in the same list are hidden from the picker; the same language can appear in both lists with different levels. Previously saved names remain available.

Choose A1–C2 to see a plain-language explanation based on the [CEFR global scale](https://www.coe.int/en/web/common-european-framework-reference-languages/table-1-cefr-3.3-common-reference-levels-global-scale); known languages also support **Native**, separately from CEFR. Edit or remove entries at any time. Rust owns the language enum, catalog, search, matching, duplicate filtering, settings validation, and app-private storage; React Native displays the returned choices.

Hold a word in the transcript to save it. Tap the bookmark button in the top bar to open **Saved words**, search your collection, play a word's source video at its caption timestamp, or remove an entry. Each word keeps its caption, language, video title, and timestamp in app-private storage, independently of the transcript cache. Saving the same word again in the same caption language keeps the original entry. Word boundaries, validation, search, and persistence run in Rust; punctuation and spacing stay unchanged in the reader.

## AI providers

Open **AI assistant** (the sparkle button), tap the model name, then **Manage providers and models**. Connections support ChatGPT plan sharing, ChatGPT / Codex, OpenAI API, Anthropic / Claude, Google Gemini, OpenRouter, xAI / Grok, Kimi Code, optional GitHub Copilot, and custom endpoints. Keep separate accounts as separate connections. Connections can be disabled, reordered, reconnected, or removed without deleting chat history.

Each connection has a persistent model library. **Refresh model list** discovers accessible models and available context, output and reasoning metadata; existing saved models and overrides are retained if discovery fails or a model disappears upstream. Add an ID manually when a provider does not offer discovery. Model settings include display name, visibility, context window, maximum output, reasoning effort, temperature, Top P, custom HTTP headers and extra JSON request parameters. The searchable chat picker lists all visible models across enabled connections, so changing a model does not require another copy of the account. The context guard uses a conservative UTF-8 byte bound; providers remain responsible for exact token accounting.

Tap **Reasoning** beside the chat's send button to expand effort choices inline in the composer. Selecting a level keeps the conversation, draft and keyboard in place. It overrides the saved model/group default and survives reopening the chat; each reply records its resolved effort after clamping to the model's advertised levels. **Default** clears the chat override, and new chats start with the saved default. Discovery supplies the selectable levels where available; GPT-6.1 Sol also has a documented fallback. For providers without effort metadata, the picker offers common settings and explains that support depends on the model. Expand **Show reasoning** on a reply to read provider-supplied reasoning separately from the effort control.

Model groups combine saved models across connections. **Ordered fallback** starts with the first available member; **Round robin** rotates the starting member for each turn. By default, failover follows HTTP 429 or 5xx responses. **Fallback on any error** also permits other failures. A response with visible answer text is never silently replaced by another model. Group reasoning defaults are clamped to a member's advertised effort levels, and the smaller of the group/model context limits applies. Replies show the actual provider/model, provider-reported usage, and expandable reasoning when supplied. Hiding a model removes it from new picker selections while preserving existing chats and explicit group membership.

**ChatGPT plan** preserves the documented [Sign in with ChatGPT](https://developers.openai.com/siwc/token-sharing-open-source/sign-in) flow under LingoPlay's own name: dynamic registration, an installation identity, browser authorization, scoped loopback callback, PKCE, state/nonce correlation and verified identity tokens. It uses the public Responses API. Plan sharing eligibility and model access are controlled by OpenAI. **ChatGPT / Codex** is a separate connection using the Codex public client registration, account model discovery and Codex Responses backend. Claude and Grok account connections also use public CLI registrations; these compatibility integrations depend on their upstream providers. OpenRouter uses its [PKCE key authorization](https://openrouter.ai/docs/guides/overview/auth/oauth) flow. Kimi Code and Copilot use device authorization; enter the displayed code in the browser and return to the app. Copilot is optional and requires accepting its unofficial-client notice before sign-in.

Browser sign-in uses Android Custom Tabs or an iOS authentication session. Authorization codes, PKCE verifiers and tokens stay in Rust/native memory. The return link contains only the pending task identity. Rust validates callback correlation and checks signed identities where supplied; xAI endpoints and signing keys come from validated OIDC discovery. Refresh is serialized per connection and preserves rotating tokens and custom model parameters. Disconnect always clears local account credentials; remote revocation is attempted where available and unconfirmed revocation is reported.

Custom connections can use Chat Completions, Responses, Anthropic Messages or Gemini protocols. HTTPS is required for remote endpoints; plain HTTP is accepted only for localhost/127.0.0.1. Keys are optional on custom endpoints. **Append /v1 automatically** adds the suffix once for API-key Chat Completions, Responses and Anthropic endpoints; it is off for existing connections and unavailable for Gemini, Azure and account sign-in. The editor previews the effective API base URL. Azure mode uses an api-key header and deployment paths; enter the resource endpoint with its api-version query, then add deployment names as model IDs.

Edit a saved API-key connection's base URL in place to keep its model library, groups and chats. Its saved key is retained for the new endpoint. The optional **User-Agent** applies to model discovery and inference; blank uses the provider default, and a model-specific header overrides it for that model's inference. Supported providers can switch between API-key and account authentication on the same connection: saving the change cancels pending work and clears the old login credentials while retaining model settings. Account tokens stay pinned to their provider's own endpoint. Request settings and key changes cancel pending requests; queued requests reject stale connection snapshots.

**Export** shares a LingoPlay JSON configuration containing connections, models, ordering and groups. **Import** accepts that JSON and creates fresh connection/group identities, preserving membership without overwriting existing accounts. API keys, account tokens, custom headers and extra request parameters are omitted; re-enter or reconnect them on the destination device. Header/body overrides use secure storage, and framework-owned model, conversation, streaming and storage fields cannot be replaced through extra parameters.

Chats and completed turns are stored in the native sandbox. Attach the current transcript explicitly to send its normalized caption text and title; remove the attachment to continue without it. Saved chats retain their provider and model selection. Closing the sheet or changing videos cancels pending chat work. Questions go directly to the selected provider; there is no LingoPlay relay. Provider configuration, routing, validation, bounded SSE parsing, authentication/refresh, model discovery, context construction and persistence live in Rust. Kotlin and Swift supply network bytes without redirects and secure storage (Android Keystore-backed AES-GCM / iOS Keychain). Credentials never enter the YouTube WebView, transcript cache, settings JSON, logs or exports. Android backup is disabled for device-bound credentials.

OpenMinis's [Android provider/model source](https://github.com/OpenMinis/OpenMinis/tree/b4c0661/src/android/app/src/main/java/com/openminis/app) was compared at commit b4c0661; LingoPlay's implementation is independently written in Rust with shared React Native presentation. On the Pixel 9 Pro, migration, the four streaming protocols, reasoning/usage, routing groups, persistent reply attribution and configuration round-tripping were verified through the native bridge and release UI. The finalized release also completed a live GPT-6.1-Sol reply through the existing ChatGPT plan connection, with usage reported correctly. Synthetic account fixtures cover callback forgery, Codex/Claude/OpenRouter/xAI login, ES256 identities, Kimi/Copilot device flows, single-flight refresh, cancellation and credential retention. Additional real-account sign-in and inference still require provider eligibility and live account checks. Protocol fixtures do not establish plan entitlement. iOS requires an Apple Silicon Mac/Xcode build and account verification after Android validation.

Known gaps from the OpenMinis source comparison:

- **Reasoning customization:** no provider-level thinking-rule editor, model-pattern rules, user-set reasoning-support flag, or per-model maximum effort override. Reasoning uses the four protocol adapters, advertised effort levels and the narrow GPT-6.1 Sol fallback rather than OpenMinis's broader vendor/model rule catalog.
- **Catalog and capability controls:** no models.dev enrichment or bundled offline catalog fallback, release-based model ranking, or editable input/output modality flags. Discovery only keeps the metadata LingoPlay currently parses from the provider response; models without effort metadata use common choices that may not all be supported.
- **Import compatibility:** no OpenMinis provider configuration or ZIP-package import. LingoPlay accepts its own credential-free JSON format through paste/text input.
- **Advanced requests:** chats currently support text and normalized transcript context. OpenMinis's provider-side image/media inputs, image-generation endpoint routing and tool-call request/response handling are not implemented.

Verification gaps remain separate from missing features: the extra account integrations have synthetic protocol coverage but still need real-account sign-in/inference checks, and iOS still needs a Mac/Xcode build and live verification.

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

Connect an arm64 Android device with USB debugging enabled. Expo generates the native Android project and starts Metro; Gradle builds the Rust library and bindings automatically. The Nix environment supplies Node 26, Rust 1.90, cargo-ndk, the Android Rust target, SDK/NDK, and Java. Node scripts run TypeScript directly using native type stripping; Node 24.15 or newer is required, with no TypeScript runner or compilation step.

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

On Android, `npm ci` also applies the Expo Video 57 PiP fixes in `scripts/patch-video.ts`: resolving the activity on cold start and hiding native dialog windows during PiP so open sheets cannot cover the video. Autolinking builds `expo-video` from source so those fixes are included instead of using its precompiled binary.

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

To profile transcript loading with Perfetto, build a diagnostic APK with the flag
below set during **both prebuild and bundling**. This enables Android shell
profiling and request-correlated async slices for cache lookup, the WebView
bridge, player captions, transcript controls, API requests, core acceptance, and
the reader's React commit. Rust adds thread slices for lock contention, parsing,
normalization, and cache I/O. Trace labels contain stage names and generated
request IDs; they contain no caption text, signed URLs, or session tokens.

```sh
nix develop path:.
EXPO_PUBLIC_LINGOPLAY_TRACE_TRANSCRIPT=1 npx expo prebuild --platform android --no-install
EXPO_PUBLIC_LINGOPLAY_TRACE_TRANSCRIPT=1 NODE_ENV=production android/gradlew -p android :app:assembleRelease -PreactNativeArchitectures=arm64-v8a
adb install -r android/app/build/outputs/apk/release/app-release.apk
node scripts/profile-transcript.ts --duration 60 --output .cache/transcript.pftrace
```

Load a transcript during the recording; use **Refresh transcript** to measure an
uncached extraction. Open the resulting file in [Perfetto](https://ui.perfetto.dev)
and search for `LingoPlay transcript:`. Page spans are timestamped when their
begin/end messages arrive in the app, so they include WebView message scheduling.
`reader.commit` ends after React commits the reader; use the frame timeline to
inspect actual presentation. Rebuild without the flag (including prebuild) to
disable shell profiling and transcript async spans.

For a timing table, run Perfetto's trace processor with
`trace_processor_shell query -f scripts/transcript-profile.sql .cache/transcript.pftrace`.

The app has been built, installed, and exercised on the Pixel: extraction, search, copy, caption seeking, inline native playback with scrollable and expanded YouTube comments, downloading separate video/audio tracks into a combined MP4, pause/resume, offline playback, background playback, and PiP. SponsorBlock skipped an actual community segment; a known ad request was intercepted with page rules enabled. The iOS Release app has been built and launched on an iPhone 17 Pro simulator on `mini`; the Rust library also builds for arm64 iOS devices. YouTube behavior on iOS remains unverified because YouTube connections time out on that host. No physical iPhone or distribution signing has been verified.

## Extraction and cache

The page bridge asks the existing YouTube player for a caption track and captures the player's fetch or XMLHttpRequest response, including requests requiring a proof-of-origin token. It clears and reselects an uncached track so a paused player fetches its text, then restores the previous selection. Default extraction and fallback requests honor the selected track, YouTube's default caption track, and the page language. Where available, it can also use YouTube's transcript control, transcript endpoint, or caption-track URL. There is no external transcript service or audio upload.

The Rust boundary accepts Android's origin-only WebMessageListener events and document-URL events from other WebView bridges, while validating the active request and video identity. Navigation cancels pending extraction and clears the reader, including single-page navigation. Raw JSON3/XML responses are parsed in Rust; transcript-renderer fallback segments receive the same validation and normalization.

Normalized transcripts are cached in app-private storage (up to 20 documents / 24 MB, with three documents in memory). Raw caption responses, signed URLs, and session tokens are not persisted. Cache write failures leave the current transcript usable. Corrupt entries are ignored on restore. Refresh bypasses the saved document.

The WebView keeps its initial source and full page height when the transcript opens; its visible region is clipped so YouTube does not mistake the smaller window for picture-in-picture and pause playback. Caption taps use the existing player's seek and play controls.

If a browser renderer terminates, pending page requests are cancelled while the transcript and native player remain available. Android disposes the dead WebView immediately; **Reload** creates a fresh view at the current address. iOS reloads its existing WebView. Recovery reloads the page rather than restoring its DOM or unsaved forms. The isolated signature runtime rejects interrupted jobs and recovers on the next evaluation, waiting for its script to be ready before accepting work. Neither path automatically replays interrupted requests.

YouTube's internal endpoints can change. Videos without captions, restricted videos, unavailable live captions, and token-protected responses may not yield a transcript. Retry after playing or reloading the video. This app reads available captions; it does not transcribe audio or translate text.

Downloads support finite AVC/AAC MP4 streams and M4A audio. Native online playback currently requires a combined video/audio stream. Live-stream downloads are unavailable. Pause/resume works while the app process survives; an interrupted process requires opening the video and retrying. Saved files and download history persist across restarts. Ad blocking hides known placements, blocks known ad requests, and skips detected video ads; YouTube changes can affect its effectiveness. SponsorBlock uses community data and continues playback if the service is unavailable.
