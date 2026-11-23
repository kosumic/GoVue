# Shared workflow ownership

Native Shell renders the shared Rust screens. The platform hosts provide WebViews,
media playback, permissions, sharing, and notifications. Workflow rules and
application state live in the Rust core.

## Downloads

`core/src/downloads/runner.rs` owns resolution, queue advancement, pause/resume,
cancellation, progress persistence, artifact assembly, publication, and cleanup.
One manager per storage root claims at most two jobs through the existing store.
Transfers use the Rust range engine. HTTPS, capability checks, MP4 remuxing, OS
publication, and Android foreground-service notifications remain native effects.
Native callbacks run outside the core and storage locks.

The downloads screen observes core updates and reloads its snapshot. After a
process interruption, the restore policy pauses incomplete jobs.

## Playback and browser boundaries

`core/src/player/session.rs` owns startup eligibility, retry cooldowns,
preparation and replacement identities, quality transitions, and remembered
preferences. The Native Shell runtime coordinates video replacement with the platform player. Stale generations cannot replace the selected video.

The synchronous `state_execute` boundary performs only bounded memory operations.
It also normalizes browser input, identifies YouTube videos, and validates layout
and playback-position messages. DOM access and geometry used for rendering stay
in JavaScript. `core/src/player/defaults.json` supplies the initial player preferences.

## Transcripts and caption audio

`core/src/transcript_session.rs` loads the selected tracks, restores cached
documents, retains a usable primary while a comparison loads, and cancels replaced
requests. Snapshots contain normalized transcripts only.

`core/src/ai/audio_requests.rs` shares in-flight caption preparation and owns media
resolution, refresh retries, cancellation, and late-route revocation. The platform player
plays the audio and reports its position.

Background workflows use the same extraction jobs as interactive requests.
`TokenRuntime` responds to native notifications and executes only BotGuard tasks;
Rust owns their protocol, network access, and request identities. Tokens, guest
cookies, and signed URLs remain in memory. The obsolete YouTube.js decoder is no
longer built or installed.

## Conversations

`core/src/ai/chat_sessions.rs` serializes sends and cancellation per UI session,
rejects superseded generations, and owns deferred game-feedback prompts. Saved
game progress updates the Rust streaming projection, so later polls include the
authoritative revisions. The UI uses the current chat session to display updates.

`core/src/ai/notifications.rs` decides whether a saved, successful reply needs an
alert, correlates a newly created chat before its worker starts, and suppresses
alerts for the foreground conversation. Native lifecycle callbacks update
foreground state. The platform hosts present generic local notifications with
opaque chat IDs and deliver taps to the app, including after a cold launch. Closing
chat keeps the response running; Stop and changing conversation/video still
cancel the owned request.

## Verification of the former React Native app

Verified on 2026-10-20 using an Android arm64 debug build and an iPhone 17 Pro
simulator Release build over SSH to `mini`. Live checks covered real YouTube
playback, fresh and cached transcripts, download pause/resume, MP4 combining,
offline playback, and caption audio preparation/playback. Chat queueing and
cancellation used a temporary loopback Responses server with dummy credentials.
The real app's player controls also opened the rendered transcript reader.

`npm run check` and `npx expo install --check` passed for that build. Rust regressions cover browser
message bounds/correlation, startup policy and stale generations, transcript
release-before-load/disposal, chat cancellation ownership, cached caption audio,
and download scheduling, Wi-Fi gating, pause/cancel, export, and file retention.
The download runner fixture advances work without JavaScript orchestration and
checks MP4/M4A temporary extensions used by native demuxers. Android device
verification was unavailable in this session.
