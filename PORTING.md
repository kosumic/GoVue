# LiTube port

Reference: HydeYYHH/Litube, `bb9ee78428a30347fc6eaf21a679f2a0b8cbc91f`
(2026-10-16). React Native owns presentation; Rust owns validation, selection,
settings, queues, navigation state, and persistence. Expo Video owns playback.
Android is the first verification target. Transcript reading, AI provider
settings, and chat are available through shared React Native screens.

The owner deferred further Cast implementation on 2026-10-17. Existing Cast
code and its recorded evidence remain; receiver verification and remaining
Cast gaps are outside the current UI-parity pass.

## Direction

Visual thesis: match LiTube's YouTube-first, dark, edge-to-edge interface, white
player controls, red progress, and compact native sheets.
Content plan: YouTube browser, embedded/fullscreen/mini player, player menus,
queue, extension settings, downloads, settings backup, and About.
Interaction thesis: controls fade on inactivity; menus dismiss and return to
the same video; fullscreen and mini-player keep one mounted video surface.

## Acceptance inventory

- [ ] YouTube-first browser and page navigation, including Chat in the bottom bar.
- [ ] Native custom player: title/author, previous/play/next, buffering/error,
  seek/buffer progress, live state, speed, quality, fullscreen, CC, More.
- [ ] Auto and explicit quality, frame-rate labels, active quality, memory.
- [ ] Audio identity, original/default preference, language switching.
- [ ] Captions on the video, language selection and caption appearance editor.
- [ ] Chapters, SponsorBlock segment list/markers/countdown and skip controls.
- [ ] Queue add/remove/reorder/clear, next/previous, repeat/pause/shuffle.
- [ ] Resize modes, gestures, hold 2x, brightness/volume, screen lock, zoom.
- [ ] Fullscreen, background audio, PiP and draggable/resizable mini-player.
- [ ] Live streams, format support and recoverable source refresh.
- [ ] Page video-card actions and playlist integration.
- [ ] Grouped extension settings, all applicable toggles, reset defaults.
- [ ] Downloads: format/audio choices, memory, manager, playlist/queue batch.
- [ ] Settings backup/import and About/diagnostics.
- [ ] Casting discovery/session/link controls and working transport (deferred).
- [ ] Android live verification of each surface and consequential action.
- [x] `npm run check` and `npx expo install --check`.

## Evidence

### SponsorBlock settings (2026-10-17)

LiTube's `extension/Extension.kt` catalog defines nine category switches and
the skip countdown, without an ad-blocking toggle or SponsorBlock master switch.
LingoPlay now defines each category row, its label, default and validation in
Rust. React Native sends individual category edits instead of rebuilding the
whole selection from its displayed snapshot. Edits share the settings write
queue; older replies cannot replace a newer category selection, reset or import.
Backups from the old sheet with SponsorBlock disabled remain effectively off
until a category is selected.

`npm run check` and `npx expo install --check` pass. A separate ARM64 Android
verification app using the real settings screen and media hook built and
installed successfully. Live switch and persistence verification is pending
because the Pixel's lock screen covers the app; no new fixture tests were added
before that live check.

### Transcript and video chats (2026-10-17)

Live Pixel 9 Pro checks confirmed Chat in the YouTube bottom bar, the full-screen
chat workspace and conversation drawer, and Transcript → AI → Speed → Quality
in the player. Opening “Me at the zoo” created an empty video conversation with
its English transcript attached. Closing and reopening chat retained that video
context without generating a reply or summary.
Each video chat also exposes **Open video**. A retained video tab is selected
without reloading its page; otherwise the watch page opens for that video.
The transcript context card opens the shared reader inside chat, with search,
copy, share, and vocabulary saving. The active response continues while it is open.
Live checks confirmed reasoning can change from Default to Low while a reply
streams; the reply completes and the selected level remains saved for the next turn.

Rust owns video-chat lookup, creation, persistence, context validation, and model
changes. Existing video conversations are reused; older duplicate files are
retained while navigation shows one conversation per video. Transcript loading
is correlated with the selected video and cancelled on replacement or dismissal.

Separate regression checks cover concurrent opens, empty chats without a
configured provider, transcript context in explicit questions, model changes
preserving history, legacy conversations, and stale transcript results. The
reasoning checks hold an active request, save a different effort for the next turn,
and verify completion preserves that preference and the original reply's effort.
React checks also cover startup changes, stale settings/completion snapshots,
and opening the context reader without reloading or cancelling the conversation. The
browser tests also cover YouTube's Trusted Types policy, bottom-bar replacement,
and returning to retained video documents without losing playlist/time parameters.
`npm run check` and `npx expo install --check` passed.

Initial audit: the existing default Media3 gear has only speed/audio menus;
quality helpers were not connected to any UI. The old catalog discarded audio
identity, FPS and non-AVC formats; native playback rejected live videos.

No parity item is complete solely because a button exists. Record live checks
and any remaining limitations here as implementation progresses.

### Android first pass (2026-10-16)

The ARM64 release build installed on Pixel 9 Pro. Live checks confirmed:

- YouTube home loads without transcript/AI/browser toolbar chrome.
- A video card opens a separate, retained watch document and native playback.
- Quality popup lists Auto, 1080p50, 720p50, 480p, 360p, 240p, 144p for the
  checked video; selecting 720p50 preserves position and playback.
- Speed popup selects 1.5x; playback and the displayed speed update.
- French captions load and render on the native video through the Rust cache.
- Fullscreen enters landscape with the same player; Back returns to embedded.
- Back from the watch page shows the mini-player over the original home page;
  Return to video restores the watch page without restarting playback.
- Add to queue and the queue sheet show the actual video and its thumbnail.

Direct `adb screencap` was used to confirm layout transitions because Argent's
automatic screenshots sometimes lagged behind its fresh accessibility tree.
Further work remains on the acceptance inventory, including full queue actions,
download parity, casting, settings, gestures, recovery and comprehensive checks.

### Second implementation pass (2026-10-16)

Additional source is now in place for:

- LiTube-style download confirmation (video/audio, quality, audio track,
  subtitles, cover, filename, attachments-only), playlist/queue selection,
  download manager filters/details, preferences, retry and file export.
- Rust-owned download configuration, validated history and transfer state,
  two concurrent items, bounded parallel byte requests (1–16 connections),
  pause/cancel, temporary-file recovery, SRT output and sandbox artifacts.
  Android supplies a foreground notification and system Save As picker.
- A separate silent media resolver, so card/batch downloads preserve the
  retained watch document and its native playback.
- Rust SponsorBlock countdown/cancel/manual skip/highlight state; original
  title lookup; deferred chapter loading; source refresh preserving position;
  replay at the end; mini-player pinch resizing/docking and persisted bounds.
- LiTube-compatible settings backup/import, functional haptics, browser cache
  clearing, fullscreen system bars, and LiTube's browser user-agent cleanup.
- Serialized signature work with cancellation, still confined to PlayerRuntime.

Normal ARM64 build fifty-eight replaced temporary Cast verification build
fifty-six after the Pixel reconnected, retaining app data. Build twenty-three caught a
missing React Android compile dependency (fixed); build thirty-three was cancelled
to complete a generation guard before building. `npm run check` and
`npx expo install --check` pass (38 JavaScript tests and 54 Rust tests).
Release WebView debugging remains disabled.

The latest installed normal ARM64 release is build sixty-six. It contains the
PiP controls and chapter-menu changes described below. Its APK contains only
`arm64-v8a` libraries and no temporary verification-driver markers.

Cast discovery and authenticated TLS adapters are connected to a Rust-owned
session/controller and source-style dialog. The implementation adds receiver
selection, shared-player links, remote controls and scoped LAN media routes.
The browser receiver passed live playback/control checks on the workstation and
through a scoped USB relay from the Pixel. Physical Cast receiver verification
and direct Wi-Fi reachability remain open.
The copied CRL root contained an extra leading `0x99`; the corrected DER matches
[Chromium's certificate array](https://raw.githubusercontent.com/chromium/chromium/main/components/media_router/common/providers/cast/certificate/cast_crl_root_ca_cert_der-inc.h)
and its self-signature verifies. SHA-256:
`8b77c60291f9d62764613083f49750b1c5caac2d9efbe5a28edf301f9d84831d`.
The owner explicitly approved casting-only LAN HTTP on 2026-10-17. See
[the approved networking boundary](docs/cast-networking.md). Native playback
continues to use its independent loopback proxy.

Further Cast work is deferred. Still to finish includes live verification
of the fullscreen image gallery (implemented zoom/save/share), physical rotation rules, source recovery and
mini-player/gesture details, complete download actions/background verification,
and systematic comparison against LiTube on a connected Android device.
iOS builds and verification require the Mac/Xcode environment.

### Android download/browser pass (2026-10-16)

The installed LiTube reference is 3.0.2; the frozen source baseline above is
3.0.3. Both are used explicitly, rather than treating the installed APK as the
newer source revision.

- The native download confirmation resolves the original "Me at the zoo"
  (jNQXAC9IVRw): 240p video, default audio, 725 KB, subtitle/cover/filename options.
- Filename editing and cover selection work. The filename remains visible above
  the keyboard. Pause on retry works and survives an app restart.
- The tenth build downloads the video plus cover using the freshly validated
  playback catalog, cached in Rust memory for at most 90 seconds of reuse.
  The manager reports Completed · 240p · MP4; both artifacts have save/share actions.
- Android Save As exported `Downloads/litube-port-test.mp4` (746,914 bytes).
  Host ffprobe confirmed H.264 320×240 video, AAC audio and 19.064-second duration.
  Local native playback works; entering from mini-player mode exposed stale mini
  controls in fullscreen. The corrected local player was checked in the twelfth:
  full player controls, fixed 240p quality and Back returning to Downloads.
- Cover Save As exported `Downloads/litube-port-test.jpg` (15,921 bytes).
  Host ffprobe confirmed a valid 480×360 JPEG.
- You → Settings → Downloads and watch-header menu → Settings both work.
  The latter requires binding UI commands to origin plus active tab while the SPA
  commits its URL; media and caption replies retain exact request/video checks.
- The player reaches Replay and displays bounded elapsed time (0:18 / 0:18).
- Download settings now match the source's radio list including 144p and 240p,
  connection slider and terminal History view. Clear-history behavior is present
  but has not been used against the device's existing records.
- A native Player preference was toggled, confirmed after an APK update and cold
  app restart, and restored to its original off value. Persistence is verified.
- Separate-page resolution still timed out through the thirteenth build. The
  installed react-native-webview 13.16.1 Android source identifies the cause:
  `WebMessageListener` passes `sourceOrigin` as the message URL, while its legacy
  bridge passes a full URL. Comparing its video ID discarded origin-only replies.
  The fourteenth uses Rust's existing trusted-origin rules for both forms while
  retaining exact request/video identity. The fifteenth completed a fresh,
  uncached retry after fixing overlapping document requests: late metadata from
  an older request cannot start a decoder that cancels the current request.
  Document epochs, final-load retries and muted player initialization are also
  in place. Card/playlist and caption attachments still need live verification.

After the live transfer, focused Rust regressions were added for download
history/recovery, owned artifact exports, excluding signed URLs from persistence,
ephemeral catalog lifetime/profile isolation, and retained-tab reload decisions.
They passed in the eleventh build's full checks.

### Shared links and gallery (2026-10-16)

- A cold Android VIEW launch from `youtu.be` opens native playback. Warm SEND
  routing selects the YouTube URL from mixed shared text, opens the NASA Posts
  page and preserves the watch document and mini-player.
- Rust now follows LiTube's page kinds and tab stack: bottom navigation resets
  intermediate pages; watch remains retained; channel/search/settings pages push.
  Android origin-only page announcements are checked against the known tab URL.
- A public NASA image post opens the native gallery. Double-tap 2× zoom, reset,
  and returning from Android Save As work. The exported
  `Downloads/litube-port-gallery-test.webp` is a valid 640×427 WebP (5,356 bytes).
  Share opens `com.android.intentresolver/.ChooserActivityLauncher`; the chooser
  was dismissed without sending the file. Multi-image paging and pinch still
  need their own live checks.
- The seventeenth build includes the source-style HSV caption editor and
  continuous caption positioning. English captions render on the native video.
  Live HSV selection updates the hex field and preview; preview dragging,
  199% text size, bold, background and reset were checked. The original default
  appearance (100%, white, bottom/center, translucent, not bold) was restored.
- Manual PiP enters Android's pinned task. Returning restores embedded playback;
  playback advanced from roughly 4:50 to 5:55 across the check. Mini-player close
  stops playback and releases its retained watch tab.
- The eighteenth adds page-driven light/dark native colors. Its appearance check
  is pending: Argent's pinch left two synthetic pointers active in Android's
  input verifier. Both LiTube and LingoPlay then ignored taps. A plain CANCEL
  does not match the two-pointer stream; the temporary helper was killed by
  Android and removed. Device lock/unlock was requested; no app routing change
  was made for this automation failure.
- The nineteenth added LiTube's queue toolbar, per-item
  downloads, clear confirmation, and long-press reorder with edge scrolling.
  Queue moves commit by video ID and offset in Rust to tolerate concurrent edits.
  These new interactions still need live verification.
- The NASA search result `jKHvbJe9c_Y` is an unavailable archive on YouTube.
  It does not establish working native live playback. Channel `/@NASA/live`
  remains in the web player; the active direct watch URL is recorded below.

### Continued player port (2026-10-16)

- NASA's anonymous live page identifies the current stream as `M3HKLzjvKPc`.
  The eighteenth native build recognizes it as LIVE but remains buffering.
  The twentieth preserves the HLS manifest's client user-agent, avoids requiring
  signature work for an HLS-only response, and adds a seek-to-live action.
  Working video playback and that action still need device verification.
- Queue layout now has LiTube's loop/clear/download/autoplay toolbar, per-item
  download and remove controls, and long-press drag ordering with edge scrolling.
  Local drag state commits one ID-relative move in Rust when dropped.
- Android physical orientation now supplies bounded, throttled sensor samples.
  Rust owns automatic fullscreen, portrait exit, manual-exit suppression and
  mini/PiP/lock guards. The twenty-first build passed that integration's checks
  and ARM64 compilation. Physical rotation remains unverified on the device.
- A one-time display sleep/wake did not clear the injected pointer verifier.
  The phone is currently locked and needs its owner to unlock it. No reboot,
  system security change or permission change was performed.

An optional ADB DevTools forward was rejected by automatic approval review due
to possible authenticated-session exposure. No forward was opened; explicit
approval was requested and ordinary UI/source checks continue.
The tenth release was also checked to have no WebView debugging socket.

### Format and platform continuation (2026-10-16)

- Build twenty-eight is installed on the Pixel. It includes Rust-generated DASH
  for Android, WebM/Opus retention and native codec capability checks. Container
  and codec families have separate adaptation sets; range requests still use
  opaque loopback routes. MP4/AAC download planning remains explicit because the
  platform remux adapters produce MP4. Decoder generation was rerun.
- The native download batch view now has aggregate progress and pause/resume/
  removal controls. Bulk pause/resume has an explicit target state, rather than
  toggling records whose state may have changed. Resolved titles/authors replace
  incomplete card metadata. Attachments-only completion no longer marks the
  watch page's video as downloaded.
- A foreground notification alone does not keep React Native timers scheduled
  after `onHostPause`. Android now anchors the existing download/mux callbacks in
  a headless task while runnable queued or active work exists. Background completion
  still needs a live transfer check; Wi-Fi waiting and process-death recovery are
  not established by this change.
- Native colors follow the page without globally overriding React Native's
  Appearance preference, which could feed the page's device-theme detection.
- iOS now has bounded, cookie-free media/image response adapters, a browser
  user-agent adapter, and a system Save As picker. Export preparation runs off
  the main thread. These Swift changes have not been compiled or run on this
  Linux host. iOS physical rotation, Cast and share-extension parity remain open.
- An anonymous, in-memory NASA probe returned API playability OK, a valid HLS
  master with six variants, a variant playlist and media bytes, all HTTP 200.
  This validates network access with the manifest client's user-agent; it does
  not establish native device playback. No signed URLs or raw responses were
  saved or logged.
- Builds twenty-five and twenty-six passed Android live-rendition selection's
  compilation and existing checks. Rust derives
  bounded quality choices from native track metadata; Expo's existing player
  applies the selection without replacing the source. The adapter checks the
  exact current source URI and clears video overrides on replacement. Device
  playback and quality changes remain pending.
- A real public VOD transport probe passed with both AAC and Opus audio, MP4
  video and WebM video. Rust generated valid DASH XML with opaque relative file
  routes, served exact 256-byte requests from upstream, and revoked the routes
  on release. The anonymous VisionOS formats used needed no signature or throttle
  evaluation. No page decoder ran on the host, and no addresses or raw responses
  were retained. This is transport evidence, separate from Media3 decoding and
  device UI verification.
- The browser adapter's former no-op for transient UI is replaced. Search and
  page-menu reports carry a native document generation and are checked in Rust
  against origin, active tab and known video. Back dismisses those overlays before
  entering mini-player or navigating. Navigation and renderer recovery invalidate
  old reports. Geometry-only updates are coalesced because this path consumes only
  overlay kind. Source-style dismissal still needs a device check.
- Shorts audio-ready/blocked reports are now correlated with the active Shorts
  document and generation. Failed autoplay suggests tapping the Short; queue-add
  failure reports show feedback. Build twenty-eight's bundle was checked against
  the current App, browser-tab and generated bridge source.

### Unlocked-device continuation (2026-10-16)

The owner unlocked the Pixel. Android still reports orphaned synthetic pointers
0 and 1 on device -1; ordinary taps remain unusable. Keyboard and D-pad attempts
did not activate the native controls. Hardware Back and Android link launches
work, allowing useful playback checks without touch. A reboot was requested to
reset the stream; unlocking was not treated as reboot consent.

- Build twenty-nine is installed. Public VOD playback still works on the current
  DASH implementation. Hardware Back returns the retained watch to mini-player.
- NASA `M3HKLzjvKPc` now plays live video on the Pixel. The previous startup copied
  the web player's position into a different native HLS timeline. Rust now returns
  no explicit initial seek for live playback, retaining Media3's default live
  position. Native video frames, Pause state and automatic adaptation from 480p to
  720p were observed. Manual quality selection and seek-to-live remain unchecked.
- A regression for live startup was added after that live check; the VOD resume
  position remains numeric. No signed manifest addresses were logged or saved.
- Build thirty ports Resize, Audio and Info from bottom sheets to centered
  dialogs. Info adds per-row copy and Copy all, plus native renderer samples for
  codecs, decoder names, frame counts, first-frame time and buffering. Rust
  validates samples and owns the rolling FPS/buffering state in memory. The
  Android adapter compiled and the APK was installed; live playback still works.
  Dialog/copy verification remains pending. The
  iOS renderer adapter remains to be ported, so those counters are unavailable
  there. Diagnostic sampling pauses while the app is in the background.

The owner explicitly chose to continue without touch verification. No reboot
will be performed. Gesture and menu-action checks remain recorded as unverified;
source comparisons, link launches, screenshots and playback checks continue.
Build thirty-one passed the checks above and ARM64 compilation. Reapplying the
Expo patch twice also passes after fixing an adapter insertion-order regression.
The lock control now lives directly on fullscreen video. Seek-bar target previews,
the source's 48 dp hit area, and chrome sizing for the player slot are implemented.
These new interactions have no touch-verification claim.

### Android publication and visual comparison (2026-10-17)

- Android 10+ download finalization now publishes verified artifacts to the app's
  `Downloads/LingoPlay` folder. Rust owns filenames, target identity, verification
  metadata, and cancellation generations. The native adapter checks MediaStore
  ownership and directory scope, copies bounded chunks, reads them back, and
  compares SHA-256 and length before exposing a finished file.
- The public Me at the zoo test MP4 (746,914 bytes) and cover (15,921 bytes) were
  published on the Pixel. Reading both files back gave exactly the earlier export
  hashes. The MP4 is H.264/AAC, 320×240, 19.063583 seconds. A cold app restart reused
  both artifacts without duplicate files. Temporary diagnostic code was removed.
- Publication boundary regressions cover stale/cancelled attempts, forged content
  URIs, checksum/length failures, interrupted completion, and retrying a newly
  resolved transfer into an existing owned target. New bytes must be verified.
- Shared controls now use 48 LiTube source vector drawables. Responsive bottom
  controls use native font measurements, including the system font scale; speed
  and quality move into More when needed, and wide layouts avoid duplicate rows.
  Source menu labels, colors, sheet limits, and drag handles are ported.
- Extension settings now use source icons, group spacing, labels, and the existing
  dislike-service descriptions. Settings export uses the native Save As picker.
  Download confirmation uses compact media chips, a landscape identity row, and
  centered radio-choice dialogs while retaining its underlying sheet.
- Mini-player geometry now follows source width ranges, inset-aware docking,
  16 dp corners, proportional transport spacing, and its close target. Rust owns
  placement and release decisions. RN coalesces pointer samples and animates the
  same mounted video surface; only settled coordinates are persisted.

Touch verification remains waived by the owner. Menu actions, drag-to-close,
resizing, and sheet dragging are implemented but have no new touch-verification
claim. Cast transport is implemented under the explicit networking
exception; physical receiver and Android browser checks remain open. Release debugging
remains disabled. iOS still needs a Mac build and verification.

### Scoped Cast transport (2026-10-17)

- The owner approved the casting-only LAN exception in `docs/cast-networking.md`.
  Native playback remains on loopback. A separate Rust listener binds the current
  private Wi-Fi address and issues a random capability for one active session.
- Rust owns receiver launch, heartbeat, request/media correlation, remote status,
  play/pause/seek/rate, replay, source replacement and revocation. Kotlin supplies
  authenticated TLS, Wi-Fi address lookup, and the active-session notification.
- Receiver routes open only after DeviceAuth succeeds, and accept media requests
  only from that receiver's IP. Shared browser sessions use a distinct capability.
  Both modes close routes on Stop, source replacement, Wi-Fi address change or
  four-hour expiry. Headers, methods, paths, ranges and concurrency are bounded.
- VOD Cast prefers AVC/AAC and generates DASH SegmentLists from MP4 sidx data,
  preserving presentation offsets. Upstream signed addresses remain in memory;
  manifests contain only session-relative parts. The browser player bundles
  dash.js 4.7.4 locally with its notices. HLS live casting is unavailable, matching
  the reference's requirement for indexed Cast formats.
- A live public-video probe on a private host interface served the shared page,
  valid DASH and seven indexed initialization/media parts. FFmpeg decoded audio
  and video through these routes. Invalid Host, capability and multi-range
  requests were rejected, and Stop revoked the listener. No source addresses,
  headers or capabilities were logged or saved.
- The bundled dash.js browser player also passed in isolated Brave against this
  Rust transport: decoded video, pause/resume, seek, and session revocation. The
  browser used a fresh temporary profile with native sandboxing enabled, no
  owner cookies, and a CDP pipe instead of a listening debugger port. This was a
  workstation transport check, not Pixel background-service evidence.
- Five Rust regressions cover protobuf framing/ambiguity, private numeric address
  policy, part-relative ranges and segment timestamp/range validation. They were
  added after the live transport probe passed.
- The Pixel displayed the source-style Cast dialog and reported no receivers.
  Its API 37 local-network permission was initially ungranted despite target SDK
  36; the adapter now checks the actual permission state, and the declared app
  permission was granted for verification. Notification permission was enabled.
- An earlier phone session served its shared page to an on-phone HTTP request.
  Desktop-to-phone TCP timed out, with the workstation route using `tailscale0`.
  A later explicit Wi-Fi socket-selection attempt failed with Android `EPERM`.
  The listener now keeps its exact private Wi-Fi address and uses Android's
  ordinary routing policy if explicit selection is denied. VPN and host routes
  were left unchanged. The fallback passed the Pixel shared-player check below.
- Cast speed now follows the reference's 0.5–2x receiver clamp, including the
  displayed speed and Info row. Failed remote rate changes do not save a new
  local preference. The shared URL uses the reference's three-line limit.
- App crash records exposed an Expo Video background-service start failure.
  Cast polling now avoids redundant notification setters and defers enabling
  services until foreground. The source patch catches Android's service-start
  rejection and foreground resume retries configuration. Long-running background
  behavior remains unverified.
- Android ARM64 builds through fifty-eight pass. Normal build fifty-four was
  installed successfully, then temporary diagnostic build fifty-six. The Pixel
  disconnected before the revised Cast build could be installed. Normal build
  fifty-eight was installed after reconnection, replacing fifty-six. It has no
  verification driver. Release WebView debugging is disabled in all these builds.
- The normal build opened the public test video and exposed the native player
  controls. Keyboard navigation reached its More control. Verification paused
  when another UID took foreground; no other app's screen was captured or read.
- After the owner confirmed the Pixel was free, keyboard navigation opened the
  normal build's More → Cast → Share controls. Android's share chooser was
  dismissed without sending anything. The scoped link stayed in memory only.
- Isolated Brave decoded the Pixel's 320×240 public test video via a temporary
  loopback-only USB media relay. Pause/resume and seek passed. A fresh manifest
  remained available after Home, with Cast foreground service/notification ID
  1048 active. Returning to the app and activating its actual Close link button
  revoked the session and stopped the service/notification. The relay and fresh
  browser profile were removed. No phone screenshots were taken.
- Direct Wi-Fi reachability, the notification's own Stop action, remote source
  continuation across queue navigation, iOS Cast adapters, and physical receiver
  validation remain open. Touch verification remains explicitly waived.

Android platform references: [local-network permission](https://developer.android.com/privacy-and-security/local-network-permission)
and [connected-device foreground services](https://developer.android.com/develop/background-work/services/fgs/service-types#connected-device).
On API 37 the adapter checks whether local-network permission is actually
granted before discovery or a session; current builds target API 36. Notification
access is required before the LAN session starts so it remains visible.

### PiP and chapter-menu continuation (2026-10-17)

- PiP now follows the source's three custom actions: Listen in background (or
  Previous when background playback is disabled), Play/Pause, and Next. Rust
  derives availability from persisted settings and the canonical queue, including
  disabled endpoints and replay after completion. The icons and English audio
  action label come from the source. Manual PiP respects the PiP preference;
  automatic entry also requires playback and platform support.
- Android renders Rust's action descriptors through immutable, package-scoped
  pending intents and a non-exported receiver. Actions require the current video,
  an enabled descriptor, and an activity actually in PiP. Rust's audio-only mode
  stays in memory. A source-correlated Media3 adapter disables the video track
  while retaining Expo's player; foreground return restores video.
- The first handoff passed on the Pixel in temporary build sixty-one: the actual
  protected headphones action closed PiP, retained the existing activity, and
  background audio advanced by 2,809 ms. Returning reused that activity. Expo's
  cached `videoTrack` property does not prove decoder enablement and is not used
  as verification evidence. Rust policy regressions were added after the live
  handoff passed.
- A subsequent Android refinement moves the pinned task directly behind Home,
  retaining the activity and mounted player. Its separate verification attempt
  did not reach PiP because the Pixel's keyguard was active. This latest transition
  remains unverified. Normal build sixty-four replaced the temporary driver;
  release WebView debugging and PiP verification code are disabled/removed.
- The chapter source audit identified missing long-press previews, current-chapter
  highlighting, and chapter-first priority. These are now implemented in shared
  React Native: right-aligned timestamps, dividers, scrolling titles, bounded lazy
  rows, measured popup width, thumbnail preview and Jump/Close. SponsorBlock rows
  appear when chapters are absent. Rust validates thumbnail CDN addresses and
  selects the current chapter. Chapter-menu live verification is pending.
- Normal build sixty-six passes `npm run check` (38 JavaScript and 54 Rust
  tests), `npx expo install --check`, and ARM64 release compilation. It replaced
  normal sixty-four while retaining app data. Temporary build sixty-five was
  compiled but not installed because unlock is still pending; all driver code
  was removed before the normal build. The newer Android keyguard dump uses
  named states, so the device helper was corrected rather than treating an
  unmatched numeric state as proof the display was off or the phone unsecured.
- Further Cast work is deferred at the owner's request. Physical PiP button
  presses, chapter long-presses, other waived touch gestures, durable partial
  download recovery, extended background behavior, and the current iOS build
  remain open. The earlier successful handoff does not establish full UI parity.
