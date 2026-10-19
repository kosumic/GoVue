# Video startup investigation and fixes

Current measurements and release decisions are in the
[ablation study](video-startup-ablation.md). It found that the startup cap
caused rebuffering even when the initial resolution was already 480p; the
release configuration now omits that cap. Native playback worked again after
the phone was reconnected later on October 18, but verification failures
returned during the study and reliable browser parity remains unmet. The earlier verification failures and
cumulative timings below describe the historical investigation before the study.

## iPad follow-up, October 19

iOS now installs the same incremental byte interface as Android. Rust can send
the first 8 KiB while the rest of its bounded range downloads. The Swift adapter
limits active handles, suspends upstream tasks when unread bytes accumulate, and
wakes pending readers on cancellation. A shared ephemeral URLSession reuses
connections across extraction and media requests, with cookies, credential
storage and URL caching disabled. Rust still owns redirects, retries and range
validation.

The installed streaming adapter alone did not resolve the reported delay. On
the physical iPad mini 6 (iPadOS 26.6), one Bunny open had a frame ready at
4.056 s but AVPlayer did not enter `playing` until 28.821 s. A later cold Bunny
control, with its saved resume position reset to zero, had a frame at 3.006 s
and entered `playing` at 10.446 s. These are different endpoints: a ready frame
does not establish that playback has started.

The iOS Expo Video patch now gives loopback HLS items a three-second initial
forward-buffer target. Once that exact current item is ready and playing, it
restores the configured forward-buffer target (normally zero, AVPlayer's
automatic policy). State is cleared on source replacement, and restoration
happens once per item. Automatic stall recovery stays enabled. Explicit buffer
settings, direct live HLS and offline files retain their configured behavior.
This does not change the quality selection. Both platforms must build
`expo-video` from source so the postinstall patch reaches the installed binary.

Keeping a one- or three-second target throughout playback was rejected after
repeat runs rebuffered. Increasing the target to 20 seconds after startup also
had stalls, including one run with very slow subsequent upstream transfers.
Restoring the normal automatic policy avoids leaving the small startup target
in effect for the whole video. Apple's documentation describes
[zero as automatic buffering](https://developer.apple.com/documentation/avfoundation/avplayeritem/preferredforwardbufferduration)
and the increased stall risk of small fixed targets.

The permanent native patch, without experimental buffer overrides, produced:

| Cold open | First frame | First playing | Later waiting |
| --- | ---: | ---: | --- |
| Bunny, first run | 3.515 s | 3.748 s | None before the next test at about 29 s |
| TED, first run | 4.197 s | 4.481 s | One approximately 0.084 s interval in 45 s |
| Bunny, repeat | 3.320 s | 3.490 s | None in 45 s |

The saved quality setting was 1080p/30 with remember-quality enabled throughout;
the available formats returned by YouTube varied (TED reported 480p in the
final native probe). These are individual device observations, not a matched
encoding distribution or a p95 claim. An earlier startup-to-automatic profile
also started Bunny in 3.712 s without waiting during its 45-second recording;
another profile run had a later five-second stall. Sustained playback under
varying CDN/network conditions remains a separate limitation.

Measurements use monotonic native timestamps from accepted browser navigation
to AVPlayer's first-frame and time-control callbacks. Diagnostic logs contain
only static phase labels, numeric fields and HTTP status codes. The temporary
probes and buffer override flags are removed from the final app. Original
history is restored after the controlled resume-position probes; app data and
saved quality settings are preserved. Recordings live in the isolated Mac
checkout's ignored `build/startup-investigation/` directory.
Selected sanitized recordings are also in the local ignored
`.cache/ios-device-startup/results/` directory. After rebuilding without probes,
the clean signed release advanced the native playhead to 13.871 s by the check
18 seconds after launch, without diagnostic environment variables. This is a
final playback sanity check, not another precise startup measurement. The
original history was then restored and verified byte for byte.

The adapters were typechecked against the iOS SDK and actual UniFFI bindings on
`mini`. A live HTTPS probe delivered the same 30,320-byte static file in three
successive streamed requests and verified bounded extraction-prefix reads.
`npm run test:ios-http` runs eight Swift adapter checks on a Mac and is included
in iOS CI. `npm run check` and `npx expo install --check` pass. Native changes
require rebuilding the app; a JavaScript reload cannot update these adapters
or the player patch.

## Outcome before the ablation

The Android fixes substantially improve the worst observed case, but **browser
parity has not been demonstrated**. Repeated cold, resumed Big Buck Bunny opens
improved from a 9.056 s median / 10.806 s maximum to 5.582 s / 6.095 s.
Brave's matched-position cohort was 5.850 s / 6.030 s. GoVue's slightly worse
maximum and its slower warm Sintel/Tears cases justified further iteration.
The newer transient startup-resolution limit produced a 5.375 s cold Bunny
open, but that single observation is not a replacement for the four-run cohort.

Later, YouTube began returning `LOGIN_REQUIRED` for the standalone guest
extractor on multiple public videos, including Bunny and Me at the zoo. The
original unmodified APK also received the same response. These attempts never
reached native source preparation or media transport and have **no valid startup
timing**. The web
page continued to play. This limits completion of the final end-to-end cohort;
it is not evidence that the transport candidate works or fails.

## Implemented changes

- Start native extraction from Rust-accepted navigation immediately. Ignore late
  callbacks for the preceding URL. Android OS links start the same bounded,
  video-correlated preparation job before React is ready.
- Read Android HTTPS ranges incrementally through opaque response handles.
  Rust can deliver the first 8 KiB before the rest of a bounded 1 MiB range
  arrives. This removes whole-body buffering without increasing the 1 MiB cap.
  Rust still validates range sizes, redirects and response lengths, owns retries
  and POST fallback, and closes every upstream handle. The original Android
  investigation did not measure iOS; see the later iPad follow-up above.
- Preserve a complete resolution ladder for Auto within the selected codec,
  independently of the deduplicated manual-quality menu. Begin Android Auto at
  an actual available height no greater than 480 (or the smallest available
  height), then remove that limit after the source-correlated first frame.
  Manual quality remains pinned. The full higher-resolution ladder remains in
  the MPD; the startup cap is not a permanent quality setting.
- Set the initial playback buffer threshold to 250 ms and retain Expo's normal
  forward buffering. Cache OS decoder capability objects. Reduce the cold
  configuration prefix from 128 KiB to 64 KiB, retaining the full-page fallback.
- Use Android's platform HTTP engine for HTTP/3 with bounded,
  demand-driven reads and an OkHttp fallback on API 34 and newer. The same
  adapter also handles bounded extraction configuration/player requests.
  No persistent HTTP cache is used.
  Brave's observed Google Video requests negotiated HTTP/3; the old OkHttp
  adapter had no QUIC support. The later same-engine QUIC-off trial negotiated
  HTTP/1.1 for media and HTTP/2 for extraction. Browser resource durations were about 0.2–0.45 s;
  slow native header waits were about 0.7–1.8 s. These are different requests,
  not a controlled transport speedup measurement. End-to-end validation of the
  HTTP/3 candidate remains pending because of the guest-extraction response.

Signed URLs, guest cookies and response bodies stay in memory. Traces include
only static phase names, protocol labels, numerical dimensions and HTTP status
numbers. The loopback proxy still exposes scoped opaque routes.

A temporary, non-exported device diagnostic fetched a public 30,320-byte static
file through the same media adapter four times. All responses negotiated HTTP/3
and delivered exactly 30,320 bytes. Header / full-response times were 752 / 846 ms
for the first request, then 183 / 247, 184 / 235 and 186 / 229 ms. This verifies
platform-engine negotiation and incremental byte delivery, not video startup or
a controlled comparison against OkHttp. Its source and APK are local diagnostic
artifacts; the normal release APK does not include the diagnostic provider.
The engine uses [Android's disabled HTTP-cache mode](https://developer.android.com/reference/android/net/http/HttpEngine.Builder#HTTP_CACHE_DISABLED)
and has no persistent storage path.

The normal release was then rebuilt with HTTP/3 for extraction as well. Cold
Bunny and warm Tears probes negotiated HTTP/3 for both watch GET and player POST
requests, all HTTP 200; Rust still reported the playability response as
verification-required. The cold first GET headers took 814 ms; subsequent
GET/POST headers took roughly 228–321 ms. No engine fallback appeared in either
trace. There was no native media source or first frame, so these numbers are
transport phases, not successful startup times. The installed normal APK's
manifest was checked to exclude the temporary diagnostic provider.

## Repeated worst-case comparison

Both new cohorts requested 6:12 into Bunny. GoVue also restores history, so its
position was reset to 6:12 before repeat launches. No app data was cleared.
Different encodings, authentication and CDN/cache choices remain possible.

| Cold, resumed Bunny | Four opens (s) | Median | Maximum |
| --- | --- | ---: | ---: |
| Original GoVue | 10.806, 7.866, 10.069, 8.043 | 9.056 | 10.806 |
| GoVue, incremental transport + early OS preparation | 4.973, 5.504, 5.660, 6.095 | 5.582 | 6.095 |
| Brave, requested 6:12 | 5.800, 5.748, 5.901, 6.030 | 5.850 | 6.030 |

Brave's default-position cold opens ranged from 3.737 to 4.701 s across the
implementation rounds. Matching a slower resumed-browser cohort alone does
not establish parity with that default-position experience.

## Later warm-video checks

These are individual checks, not repeated final distributions. Sintel's listed
Brave observation was from iteration 3; Bunny and Tears were from iteration 5.

| Warm open | GoVue with startup cap | Brave | Result |
| --- | ---: | ---: | --- |
| Bunny, around 6:12 | 3.076 s | 3.112 s | Similar in this observation |
| Sintel | 4.016 s | 3.302 s | GoVue slower |
| Tears of Steel | 4.372 s | 2.825 s | GoVue slower |

Sintel's startup-limit trace confirmed a first native height of 364; Tears was
356 and Bunny was 480. Each trace immediately released the cap after the first
frame. Sustained native playback and a later quality ramp on the final HTTP
engine candidate still need live verification.

## Validation and exclusions

`npm run check` passed: TypeScript, 130 JavaScript tests, Rust formatting,
Clippy and all Rust tests. Added coverage checks incremental initial delivery,
contiguous bounded ranges, truncated/oversized responses, invalid redirects,
handle cleanup, complete Auto ladders, manual quality, actual startup heights,
accepted-navigation ordering and reuse of the OS preparation job.
`npx expo install --check` reported dependencies up to date. Android ARM64
release builds and release lint passed. Device frame submission is measured
with Perfetto, separately from fixture tests.

The implementation rounds contain 39 launch attempts so far in addition to
27 valid baseline opens. One iteration-3 launch overlapped APK installation and
is explicitly invalid; four iteration-6 and two iteration-7 attempts failed
before native playback. Screenshots confirm verification-required responses
on Bunny, Me at the zoo and Tears. Two iteration-1 warm trace windows overlap
and are excluded from the repeated comparison. No population p95/p99 is claimed.
A later Brave cold control still played the requested film at 6:12 in 5.879 s.
The final HTTP-engine candidate is not included in the timing tables above.

Earlier screenshots also show a floating comparison-player thumbnail. Later
transport probes explicitly stopped Brave and the official YouTube app. Its
incremental cost in the earlier timing cohorts was not isolated. The next
end-to-end cohort should keep other players stopped, repeat both cold and warm
opens across the same films, and verify visible playback and Auto's later
quality increase. The verification response does not establish whether the
restriction is tied to the network/IP or the standalone guest client. No VPN,
network route or authentication settings were changed during this work.

Local artifacts are under `.cache/startup-investigation/iteration{1,2,3,5,6,7}/`.
Each directory contains its local repeat runner, launch clocks, Perfetto traces,
phase CSVs and JSON results. APKs and diagnostics are ignored by Git.

## Original baseline investigation

Measured on 2026-10-18 using the existing installed GoVue release on a Pixel 9
Pro running Android 17. The expanded investigation contains **27 timed opens
across six videos**, plus one excluded measurement attempt. The original set
had 13 timed opens; the follow-up added 14: eight GoVue, three official YouTube,
and three Brave measurements. Cumulative counts are 15 GoVue, six YouTube, and
six Brave opens. Playback code and preferences were unchanged; no app was
installed and no app data was reset for these measurements.

The repeatable worst case was Big Buck Bunny (`aqz-KE-bpKQ`). In the observed
GoVue state it resumed mid-video at **Auto 1440p60**, using the hardware VP9
decoder at 2560×1440. Its startup delay was substantially larger than the
other added long videos.

| Big Buck Bunny | GoVue | Official YouTube | Brave |
| --- | --- | --- | --- |
| Cold process, follow-up runs | 7.866, 8.043, 10.069, 10.806 s | 1.525, 1.721 s | 3.879 s |
| Cold median / maximum | **9.056 / 10.806 s** | 1.623 / 1.721 s | 3.879 / 3.879 s |
| Warm process, both rounds | 5.611, 7.589 s | 0.917 s | 1.854, 2.926 s |

These are observations under each app's existing quality and resume behavior.
Quality, codecs, resume offsets, authentication, CDN selection, and internal
caches were not forced to match. The table measures the experience under those
conditions; it is not a comparison at an identical encoding and position. The
small sample supports reporting observed ranges, medians, and maxima, not a
population p95 or p99. Video duration alone does not explain the slow case.

The other added long-video opens in GoVue were Sintel (3.511 s), Tears of Steel
(3.077 s), and Elephants Dream (3.268 s). Sintel took 0.971 s in YouTube and
2.544 s in Brave. The full six-video set also includes Me at the zoo and a
45-second video selected from its search results in the original round.

The slowest GoVue run, `govue-big-cold-1`, breaks down as follows:

| Stage | Elapsed stage time | Cumulative time |
| --- | ---: | ---: |
| Launch to extraction start | 1.910 s | 1.910 s |
| Media extraction | 1.737 s | 3.646 s |
| Extraction result to native source preparation | 0.144 s | 3.791 s |
| Native source preparation to first frame | **7.015 s** | **10.806 s** |

The configuration-prefix request accounts for 1.158 s of extraction, not an
additional stage. Fourteen upstream header requests started after native
preparation and before its first frame. Their responses were all HTTP 200;
there was no 403, redirect, or retry sequence in this interval. Successive
completed large-body reads took up to approximately 0.9 s. The native VP9
decoder was configured around 4.784 s after launch, with the configure section
taking about 3 ms. The longest core-lock wait in the complete trace was 22 ms.
These measurements place the dominant delay in media delivery and buffering,
rather than seconds spent constructing the decoder or waiting for the core
lock.

This tail reproduced. The other three cold Bunny opens spent 3.882, 4.089,
and 6.188 s after native preparation. The warm reopen still spent 5.744 s in
that stage despite extraction taking only 0.266 s. The added long films spent
1.223–1.498 s there. The next optimization should target the high-quality,
resumed Bunny case and preserve those faster cases.

The pre-change source identified these candidates (line numbers refer to that version):

- `core/src/transport.rs:278`: each `Body::fill` waits for a complete bounded
  upstream range before exposing its bytes. The range size begins at 32 KiB
  for every incoming media request and doubles to 1 MiB. Audio and video can
  load in parallel, but successive ranges within one response wait for the
  preceding range.
- `modules/govue-core/android/src/main/java/io/github/kaminarios/govue/coremodule/GoVueCoreModule.kt:49`:
  `NativeMediaHttp` reads the whole bounded response body before returning it
  to Rust. The local proxy streams between ranges; the HTTPS adapter does not
  stream within a range. Raising the initial range size alone could therefore
  restore a large initial buffering delay, while smaller ranges increase
  round trips. Overlapping bounded fetches and consumption, and reusing
  initialization/index bytes, are candidates for an A/B implementation.
- `App.tsx:203` and `App.tsx:397`: accepting a browser target initiates page
  navigation, while `watch` is set from the WebView URL callback. This remains
  a gate before native extraction. Running-app extraction started 1.042–1.519 s
  after URL delivery in the added warm tests. Starting or claiming native
  work directly from an accepted target is another candidate; keep document
  generation, cancellation, and response correlation checks.
- `core/src/extraction.rs:510`: cold extraction fetches configuration and
  makes a separate player request. Bunny's four cold prefix requests took
  1.158–1.363 s; the warm Bunny extraction took 0.266 s with reused
  configuration. Configuration reuse helps but does not remove the worst
  native delay.
- The trace also contains web decoders at 640×360 alongside native decoding at 2560×1440. The incremental cost was not isolated. Later inspection found that the copied `player-hook.js` watch-tracking routine is not called by its current entry point; it must not be treated as a proven cause.
- `src/media/useMediaBrowser.ts:712`: optional original-title lookup is
  awaited before preparation. It was not responsible for the worst observed
  run: the entire post-extraction handoff took only 0.144 s.

High initial quality and the resume position are important conditions to hold
constant in a follow-up A/B. The measured network/buffering stage is the main
target, but the portion attributable to quality selection versus the transport
has not yet been isolated. No performance fix had been applied during this baseline phase.

Every follow-up attempt is listed below. Times are seconds from launch dispatch
to the first video frame; a warm process can still fetch uncached metadata.

| Case | App | Video | Process | First frame | Native preparation → frame |
| --- | --- | --- | --- | ---: | ---: |
| govue-big-cold-1 | GoVue | Bunny | Cold | 10.806 | 7.015 |
| govue-sintel-warm | GoVue | Sintel | Warm | 3.511 | 1.265 |
| govue-tears-warm | GoVue | Tears of Steel | Warm | 3.077 | 1.498 |
| govue-elephants-warm | GoVue | Elephants Dream | Warm | 3.268 | 1.223 |
| govue-big-warm-2 | GoVue | Bunny | Warm | 7.589 | 5.744 |
| govue-big-cold-2 | GoVue | Bunny | Cold | 7.866 | 4.089 |
| govue-big-cold-3 | GoVue | Bunny | Cold | 10.069 | 6.188 |
| govue-big-cold-4 | GoVue | Bunny | Cold | 8.043 | 3.882 |
| youtube-big-cold | YouTube | Bunny | Cold | 1.721 | — |
| youtube-sintel-warm | YouTube | Sintel | Warm | 0.971 | — |
| youtube-big-warm | YouTube | Bunny | Warm | Excluded | — |
| brave-big-cold | Brave | Bunny | Cold | 3.879 | — |
| brave-sintel-warm | Brave | Sintel | Warm | 2.544 | — |
| brave-big-warm | Brave | Bunny | Warm | 1.854 | — |
| youtube-big-cold-2 | YouTube | Bunny | Cold | 1.525 | — |

The excluded warm YouTube attempt reused the previous decoder and lacked a
new configure boundary for identifying the new video's first frame. Frames
from the previous video were already arriving at trace start; treating those
as startup would produce a false near-zero result. This is an unavailable
timing, not evidence of playback failure. A second cold YouTube run replaced
it. Brave's cold renderer was initially named `<pre-initialized>` in Perfetto;
its PID and UID were verified against the live Brave privileged process before
including its frame timestamp.

The start clock is `/proc/uptime` immediately before `am start` or the original
card tap, with 10 ms clock resolution plus dispatch overhead. GoVue uses its
source-correlated `GoVue playback: first frame` marker. YouTube and Brave use
the new decoder's `renderOutputBuffer` event, excluding old decoder frames.
This endpoint is native frame submission, not screenshot-based detection of
the first visible pixel. The original warm browser-document check used
`requestVideoFrameCallback` and is separately identified in the earlier report.
Each added trace covers 20 seconds. Trace results contain phase labels,
durations, and HTTP status codes, without signed URLs or session tokens.
Warm request counts exclude requests that began before preparation of the new
native source, avoiding attribution of the preceding video's requests.

The phone was USB powered. Battery temperature rose from 36.7°C at the start
to 38.2°C later, and global thermal status changed from 0 to 1. Late comparison
runs did not have identical thermal conditions to the earlier GoVue runs;
the effect was not quantified. No app-data or browser-cache reset was used.

Local artifacts are retained under `.cache/startup-investigation/round2/`:
`results.jsonl` includes all 15 attempts, `summary.json` contains aggregates,
`conditions.json` records phone and observed quality conditions, and each case
has its launch clock, `.pftrace`, timing CSV, and JSON result. `run.mjs` is the
local repeat runner; it requires the project's Android toolchain and the
existing `.cache/trace_processor_shell`. The original report is preserved as
`.cache/startup-investigation/round1-report.md`. These local recordings and
helpers are ignored by Git. Only this report is intended for Git.
