# Standalone YouTube extraction

Rust owns YouTube configuration parsing, player requests, caption-track selection,
caption URLs, token protocol requests, validation, normalization and caches.
`extract_start`, `extract_poll`, `extract_token` and `extract_release` expose a
bounded asynchronous job so network waits never hold the shared core lock.
Playback, captions and download resolution use this boundary. They do not load a
hidden YouTube page, capture player requests, click transcript controls or fall
back to page extraction.

Android and iOS provide bounded HTTPS byte adapters with redirects, automatic
cookie storage and disk caching disabled. Rust keeps the extraction's guest
cookies only in memory, scoped to YouTube requests. It downloads a watch document
as data to obtain the current visitor/client context, then requests a VISIONOS player response. Client
parameters follow yt-dlp's YouTube extractor at commit `8973412`. Direct formats
feed the existing Rust media validator, proxy and download transfer engine.
Native adapters continue to provide MP4 remuxing and OS file publication.

Protected captions use a BotGuard challenge fetched and verified by Rust. A
separate local renderer executes the challenge and mints a video-bound token
using BgUtils. Its local HTML uses a YouTube base origin for attestation without
loading a page. Its CSP disables network and external resources, and its cookie
API is blocked. It receives neither caption text nor signed media/caption URLs, and returns only snapshot
and token strings correlated with the active extraction task. It never loads a
YouTube document. Player signature code must continue to use PlayerRuntime;
the token renderer is not a general player-code evaluator.

On iOS, WebKit reports the local HTML's YouTube base URL as its initial
navigation. The renderer permits that first top-level local load, then rejects
subsequent HTTPS navigation, including navigation to the same base URL. A
replacement renderer gets a fresh initial-load allowance.

The Pixel verification extracted 11 streams and two caption tracks from the
public video `jNQXAC9IVRw`, played it through Expo Video, downloaded a 309,288-byte
audio file using the Rust transfer engine, and parsed six English caption lines.
The download probe used a separate temporary store. An iPad mini live check
also extracted six English caption lines from that video after fixing the
verification renderer's initial local navigation. iOS playback and downloads
were not covered by that transcript check.

Player responses and signed catalogs remain in a bounded memory cache for 90
seconds (three videos). Only normalized transcripts enter the existing native
transcript cache. Cancelling or changing videos discards pending UI results and
releases the Rust job; in-flight HTTPS calls finish within their bounded timeout.
A browsing-renderer failure does not cancel independent extraction.

Tapping a video card starts media extraction before the watch page renders.
Rust validates the source tab, document generation, origin and selected watch
URL before creating one preparation job per native store. Playback takes over
that same job, including work still in flight, rather than starting another
watch/player request. A different selection replaces the preparation; navigation
away cancels it. Explicit refreshes, failed preparations and preparations older
than 90 seconds start a new extraction. Preparation uses the same standalone
guest extractor and keeps all signed addresses in memory. Android traces expose
`GoVue extraction: prepare media`, `claim preparation` and `media` sections for
checking the handoff without recording addresses or session data.
Native playback starts from the selected watch URL after preferences load;
it does not wait for the web player's readiness event. That event still updates
the embedded surface and provides a fallback trigger without duplicating work.
Rust-accepted navigation selects the native video before the WebView URL
callback. Android also starts the same preparation job from a validated OS
watch link while React starts; playback claims that work after navigation is
accepted. A late callback for the preceding URL cannot select the old video.
Playback extracts guest configuration from a bounded 64 KiB page prefix and
reuses that configuration and its guest cookies in memory for 90 seconds across
videos. A missing prefix context falls back to the full page. Transcript loading
still requests full web caption metadata when it was absent from the prefix;
that optional data no longer holds up video startup.

The Rust media transport follows at most four CDN redirects per bounded range,
validating each target as an HTTPS Google Video address. Adapters return the
Location header without following it themselves. A successful target stays in
the source's memory for later ranges, so a 302 does not repeatedly request the
original edge. Redirects preserve the byte bounds; 307/308 also preserve POST.
Android traces include HTTP status numbers and separate header/body phases,
plus player preparation and first-frame markers.
Android provides incremental HTTPS response handles. Rust requests at most
1 MiB at a time and validates the first 8 KiB before sending success headers;
the player can consume those bytes while the rest of that range arrives.
Every callback read is at most 64 KiB. Rust rejects truncated and oversized
bodies and closes the handle on completion, cancellation or failure. Platforms
without this optional adapter retain the initial 32 KiB range and growth to the
1 MiB cap.

Android Auto playback keeps a complete resolution ladder within the selected
codec. Its initial size limit uses the highest available height up to 480, or
the smallest available format. The source-correlated first-frame callback
removes that temporary limit so Auto can increase quality normally. Manual
quality selection remains pinned to the requested format. The initial playback
buffer threshold is 250 ms; normal forward buffering remains enabled.

Machine-translated captions are requested immediately. If a verified request
returns 429 while the in-memory guest session is younger than 75 seconds, Rust
waits for the remaining session age and retries that request once, following the
[yt-dlp session-age workaround](https://github.com/yt-dlp/yt-dlp/pull/15709).
Original captions, cached transcripts and successful translation requests have
no artificial delay. This retry applies only to younger sessions.
Refreshing player metadata reuses the cached guest cookies and their age;
changing the visitor cookie resets that age.
The wait is cancellable and extraction jobs allow up to 180 seconds so the wait
does not consume the verification/network budget. Unmarked caption requests
that return 403 or 429 attempt verification once. If a verified translation
still returns 429, the reader reports the temporary limit and preserves the
original transcript for Retry. Guest cookies and age remain in memory only.

This initial standalone extractor uses a guest VISIONOS client. Sign-in-required
videos and videos unavailable to that client fail explicitly. Formats requiring
signature/throttle evaluation are not selected. There is no page fallback.
