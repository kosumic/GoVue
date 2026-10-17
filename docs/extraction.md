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

The Pixel verification extracted 11 streams and two caption tracks from the
public video `jNQXAC9IVRw`, played it through Expo Video, downloaded a 309,288-byte
audio file using the Rust transfer engine, and parsed six English caption lines.
The download probe used a separate temporary store. iOS live verification has
not been performed for this change.

Player responses and signed catalogs remain in a bounded memory cache for 90
seconds (three videos). Only normalized transcripts enter the existing native
transcript cache. Cancelling or changing videos discards pending UI results and
releases the Rust job; in-flight HTTPS calls finish within their bounded timeout.
A browsing-renderer failure does not cancel independent extraction.

This initial standalone extractor uses a guest VISIONOS client. Sign-in-required
videos and videos unavailable to that client fail explicitly. Formats requiring
signature/throttle evaluation are not selected. There is no page fallback.
