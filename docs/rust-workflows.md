# Shared workflow ownership

React Native renders snapshots and adapts view lifetime, permissions, sharing,
WebView execution, and Expo Video calls. Workflow rules and application state live
in Rust. The native adapters expose bounded platform effects through UniFFI.

## Downloads

`core/src/downloads/runner.rs` owns resolution, queue advancement, pause/resume,
cancellation, progress persistence, artifact assembly, publication, and cleanup.
One manager per storage root claims at most two jobs through the existing store.
Transfers use the Rust range engine. HTTPS, capability checks, MP4 remuxing, OS
publication, and Android foreground-service notifications remain native effects.
Native callbacks run outside the core and storage locks.

The React hook subscribes to `coreUpdate` notifications and requests snapshots.
It has no polling timers or transfer orchestration. Android's service no longer
starts a headless JavaScript task. Work remains subject to OS process lifetime;
after process interruption, the existing restore policy pauses incomplete jobs.

## Playback and browser boundaries

`core/src/player/session.rs` owns startup eligibility, retry cooldowns,
preparation and replacement identities, quality transitions, and remembered
preferences. React serializes and executes Expo Video effects, then acknowledges
successful replacements. Stale generations cannot replace the selected video.

The synchronous `state_execute` boundary performs only bounded memory operations.
It also normalizes browser input, identifies YouTube videos, and validates layout
and playback-position messages. DOM access and geometry used for rendering stay
in JavaScript. `core/src/player/defaults.json` supplies the same initial defaults
to Rust and React Native.

## Transcripts and caption audio

`core/src/transcript_session.rs` loads the selected tracks, restores cached
documents, retains a usable primary while a comparison loads, and cancels replaced
requests. Snapshots contain normalized transcripts only.

`core/src/ai/audio_requests.rs` shares in-flight caption preparation and owns media
resolution, refresh retries, cancellation, and late-route revocation. Expo Video
continues to own audio playback and presentation of its progress.

Background workflows use the same extraction jobs as interactive requests.
`TokenRuntime` responds to native notifications and executes only BotGuard tasks;
Rust owns their protocol, network access, and request identities. Tokens, guest
cookies, and signed URLs remain in memory. The obsolete YouTube.js decoder is no
longer built or installed.

## Conversations

`core/src/ai/chat_sessions.rs` serializes sends and cancellation per UI session,
rejects superseded generations, and owns deferred game-feedback prompts. Saved
game progress updates the Rust streaming projection, so later polls include the
authoritative revisions. React applies revisioned deltas and guards its own
rendering callbacks against a dismissed or changed view.

## Verification

Verified on 2026-10-20 using an Android arm64 debug build and an iPhone 17 Pro
simulator Release build over SSH to `mini`. Live checks covered real YouTube
playback, fresh and cached transcripts, download pause/resume, MP4 combining,
offline playback, and caption audio preparation/playback. Chat queueing and
cancellation used a temporary loopback Responses server with dummy credentials.
The real app's player controls also opened the rendered transcript reader.

`npm run check` and `npx expo install --check` pass. Rust regressions cover browser
message bounds/correlation, startup policy and stale generations, transcript
release-before-load/disposal, chat cancellation ownership, cached caption audio,
and download scheduling, Wi-Fi gating, pause/cancel, export, and file retention.
The download runner fixture advances work without JavaScript orchestration and
checks MP4/M4A temporary extensions used by native demuxers. Android device
verification was unavailable in this session.
