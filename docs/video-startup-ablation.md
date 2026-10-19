# Android video startup ablation

## Status and interpretation

The controlled screen found and removed a regression: the temporary startup
resolution cap caused a playback interruption immediately after the first frame.
The promoted, cap-disabled ARM64 APK is installed on the phone. Its SHA-256 is
3f43d859aacdb329282e6d9dff7aa14f885889f5f5b956d83d53daaced868ea9,
identical to the measured no-startup-cap APK.

This is a **partial ablation study**. Six feature-removal comparisons completed
four cold repetitions each. The study contains 54 timed measurement attempts:
42 successful starts (41 native and one Brave), three startup failures, and
nine excluded pilot/interrupted measurements. Primers, preflights and the
60-second diagnostic are separate. Native availability then deteriorated from
media HTTP 403 responses to sign-in/verification-required responses. The latter
also occurred with QUIC disabled and during the OkHttp preflight. Later Sintel
and Tears probes produced no native frames. The remaining comparisons and the
controlled warm cohorts could not be completed.

**Browser parity is not established.** A concurrent Brave control opened Bunny
at requested 6:12 in 4.948 s. The later promoted native cohort had three starts
at 3.737, 4.474 and 5.167 s, plus one failure with no frame in 20 seconds. A
median calculated only from its successful starts cannot hide that failure.
No network, VPN, authentication or display settings were changed.

The earlier iteration tables are cumulative before/after measurements and
cannot identify individual contributions. The 3.657 s reconnect preflight is
also not an ablation result; its actual resume offset was not recorded.

The first four controlled full-build cold opens were 3.529, 3.692, 4.899 and
3.868 s. That pilot block changed from thermal status 1 to 2 without per-trial
thermal readings; it is retained as an observation and excluded from causal
comparisons. The initial warm pilot also lacked a fixed primer offset.
Subsequent trials record temperature and thermal status individually. Full
controls are repeated near the ablations, and thermal strata must be compared
separately when conditions differ.

The QUIC-off variant actually negotiated HTTP/1.1 for the tested CDN media
requests and HTTP/2 for extraction. Label it as QUIC on/off in this environment;
it is not a pure HTTP/3 versus HTTP/2 media comparison.

Each primary comparison removes one feature from the same full implementation.
The measured effect is conditional on the other enabled features. Effects must
not be added together, and a small-sample observed maximum is not a population
tail estimate. Negative latency differences can reveal a regression.

### Startup-cap regression found by the study

Four controlled cap-disabled starts were 4.937, 4.704, 5.366 and 4.804 s to
the first frame; all began at 480p and none rebuffered during the 20-second
recording. The adjacent full-build control also began at 480p, with a 4.824 s
median first frame, but all four starts rebuffered. Its median completed
post-start stall was 2.693 s. The first interval containing five uninterrupted
seconds of playback began at a median 7.883 s, versus 4.971 s with the cap
disabled. Both blocks were at thermal status 3, around 44 °C.

The trace shows the full build releasing its size constraint immediately after
the first frame, then returning to BUFFERING. In the cap-disabled trials the
initial rendition was already the same 480p, with no parameter change at that
point. This isolates a regression hidden by first-frame-only timing. The
input-format marker alone does not establish visible output quality.

The subsequent 60-second diagnostic had no rebuffering. Input formats advanced
from 480p30 to 720p60 at 18.651 s, then 1080p60 at 42.332 s after launch.
Screenshots confirmed visible native playback. That diagnostic included slow
ADB screenshots and is excluded from the primary timing distribution. The
release configuration now omits the startup-limit field; the native diagnostic
hook remains available to reproduce the capped reference in experiment builds.

The five-second measure is retrospective: find the first source-correlated
`playing true` interval lasting at least five seconds inside the trace. It is
the start of that interval, not the end of the five-second confirmation period.
Missing observation time is reported as missing, not a zero or a timeout value.

## Completed cold comparisons

All native rows below requested Bunny at 6:12 and verified an actual initial
position of 372.477 s, Auto quality and 480p. The reference APK was unchanged
between control blocks. Status 3 controls and ablations were around 44 °C;
the earlier status 2 transport comparison was around 43 °C. These thermal
strata are kept separate. Cold means a stopped process, with app data and
ordinary browser/OS caches preserved.

| Block / variant | Thermal status | Started / attempted | First-frame median / max (s) | First uninterrupted 5 s begins, median (s) | Post-start stall median (s) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Full control, earlier | 2 | 4 / 4 | 4.124 / 4.138 | 6.390 | 2.060 |
| QUIC disabled, earlier | 2 | 4 / 4 | 5.055 / 5.698 | 8.475 | 2.744 |
| Buffered bodies + original 32 KiB range ramp | 3 | 4 / 4 | 5.721 / 5.848 | 7.776 | 1.921 |
| Full control before cap comparison | 3 | 4 / 4 | 4.824 / 5.621 | 7.883 | 2.693 |
| Startup cap disabled | 3 | 4 / 4 | 4.870 / 5.366 | 4.971 | 0.000 |
| Early preparation disabled | 3 | 4 / 4 | 6.934 / 7.337 | 9.436 | 2.430 |
| Full control repeated | 3 | 4 / 4 | 4.820 / 5.627 | 7.532 | 2.550 |
| Original 128 KiB prefix | 3 | 4 / 4 | 4.928 / 5.854 | 7.759 | 2.532 |
| Default playback-buffer threshold | 3 | 4 / 4 | 4.998 / 5.312 | 7.578 | 2.721 |

The largest supported timing effect is early preparation: disabling the
combined OS/accepted-navigation preparation added about 2.11 s relative to
the nearby status 3 reference medians. The buffered/range-ramp ablation was
about 0.90 s slower to the first frame than the following matched-temperature
control. This is the **combined range/delivery strategy**, not an isolated
incremental-streaming effect. Neither comparison estimates an in-page card-tap
effect independently of OS-link delivery.

QUIC disabled was about 0.93 s slower than the nearby status 2 reference.
This is conditional evidence for the combined extraction/media transport
choice on this connection. Later verification failures occurred with QUIC off
too, so they cannot be attributed to QUIC. The 0.11 s prefix and 0.18 s buffer
differences are small relative to the approximately 0.8–1.1 s spread within
nearby controls: this screen does not establish a latency benefit for either.

### Raw first-frame values

Values are seconds, in execution order. These are observed maxima, not p95/p99.

| Block | Four first-frame values |
| --- | --- |
| Full, thermal status 2 | 3.943, 4.121, 4.138, 4.128 |
| QUIC disabled, thermal status 2 | 4.514, 4.799, 5.698, 5.310 |
| Buffered / 32 KiB ramp | 5.848, 5.706, 4.912, 5.737 |
| Full before cap ablation | 4.364, 4.509, 5.621, 5.140 |
| No startup cap | 4.937, 4.704, 5.366, 4.804 |
| No early preparation | 6.393, 7.337, 6.945, 6.923 |
| Full repeated | 5.121, 4.518, 5.627, 4.508 |
| 128 KiB prefix | 5.169, 4.681, 4.686, 5.854 |
| Default buffer | 4.337, 5.312, 5.055, 4.940 |

### Failures and incomplete comparisons

- The legacy ladder's first controlled measurement selected AVC and received
  repeated media HTTP 403 responses. It produced no frame within 20 seconds.
  A later production VP9 trial failed the same way. Thus the legacy-ladder
  failure cannot be attributed to that feature from this screen.
- The production recheck completed three of four opens without post-start
  stalls; its fourth open received media 403s and never rendered a frame in
  the 20-second window. The existing recovery later reused the cached catalog
  rather than obtaining fresh media addresses. That recovery behavior is an
  identified follow-up issue; it was not changed in this study.
- The later QUIC-off block completed two opens (4.776 and 4.992 s), then failed
  before native media loading with a sign-in/verification-required response.
  Its fourth scheduled measurement was not run. The OkHttp block failed its
  discarded primer with the same visible verification message, so it has no
  measured timing cohort. HTTP 200 on the player API is not proof of an OK
  playability response.
- Decoder-capability-cache removal and the uncapped legacy-ladder interaction
  APKs were built but not measured. The controlled warm protocols were prepared
  but not executed. The earlier warm pilot is excluded because primer offsets
  and cache age were not independently controlled.
- Four early cold pilot observations are excluded because thermal status
  changed from 1 to 2 without per-trial readings. One later trial was excluded
  after the phone switched apps and its seek reset missed. Both valid and
  excluded observations remain in the local data.

The current VISIONOS client fields match the upstream
[yt-dlp client configuration](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/youtube/_base.py).
This check does not identify the cause of the verification restriction, and
does not establish an HTTP-engine, IP, cookie or client-version explanation.

## Variant matrix

| Profile | Difference from full |
| --- | --- |
| full | Historical study reference: all optimizations, including the regressing cap |
| production | Promoted release configuration: same reference with cap disabled |
| no-streaming | Buffer complete upstream bodies; restore initial 32 KiB ranges |
| streaming-32k | Keep incremental delivery, begin ranges at 32 KiB |
| buffered-1m | Buffer bodies, begin ranges at 1 MiB |
| no-early | Remove OS preparation and immediate selection from accepted navigation |
| no-ladder | Restore the former cross-codec resolution deduplication for Auto |
| no-startup-cap | Remove the temporary initial resolution limit |
| no-auto | Restore both former ladder construction and uncapped initial selection |
| default-buffer | Use Expo's default initial playback buffer threshold |
| no-http3 | Disable QUIC in the same platform HTTP engine for extraction and media |
| no-http3-media | Disable QUIC only for media requests |
| no-http3-extraction | Disable QUIC only for configuration/player requests |
| okhttp | Use OkHttp for extraction and media; separates engine choice from QUIC |
| prefix-128k | Restore the 128 KiB configuration prefix |
| no-capability-cache | Restore the original per-batch decoder capability scan |

The four streaming/range-size profiles can form a 2×2 comparison. Full,
no-ladder, no-startup-cap and no-auto form the Auto comparison. The four QUIC
profiles can separate extraction from media transport. Negotiated protocols must
be verified from traces: merely enabling QUIC is not evidence of HTTP/3 use.

The initial screen prioritizes no-streaming, no-early, no-startup-cap,
no-ladder, default-buffer, no-http3, prefix-128k and no-capability-cache.
The Auto interaction control was built but could not be measured. Separate
range-size and transport-engine controls are available; the OkHttp control
failed its preflight. The no-streaming result describes the combined
delivery/range strategy.

## Device protocol

- Use the same phone, connection, VPN route, app preferences and movie offsets.
  Stop Brave and official YouTube before native trials. Record battery
  temperature and thermal status; avoid mixing changed thermal conditions.
- Primary case: cold process, Bunny at 6:12, Auto quality, four repetitions per
  profile. Reset history to the same position before every launch; the link's
  requested position alone is insufficient because history can take precedence.
- Check warm cached-target starts on Sintel and Tears separately, with a fixed
  primer sequence. Each independent trial starts a fresh process, primes the
  target, resets its history to zero, primes Bunny at 6:12 and pauses/resets the
  primer to the same position before timing the target. The target is revisited
  within the 90-second metadata cache lifetime; trace assertions verify a cache
  hit. Do not combine cached and uncached warm observations.
- Keep stable APKs for each profile. Complete installation before recording.
  Interleave full controls with ablations, counterbalance profile order, and
  compare local blocks rather than an early full run against a much later
  ablation. Recheck the largest apparent effects with reversed order.
- The timed interval starts at phone uptime immediately before launch dispatch.
  Use the source-correlated native first-frame marker. Confirm the actual first
  position, first resolution and codec; exclude reused/incorrect-source frames.
- Also record time to a common quality threshold, post-start buffering and
  resolution after 10–30 seconds. A faster low-resolution first frame alone
  does not establish better overall startup. Verify visible playback separately.
- Separate a cached-target revisit from a warm process opening an uncached
  target. For the latter, reset the target's history in a primer, cold-start
  Bunny to clear process-local extraction metadata, then time the target
  without stopping that warmed process. Assert a cache miss from the trace.
- Count missing frames, errors and verification responses as failures; never
  replace them with the trace-window duration. Stop the block if verification
  recurs. Preserve exclusions and raw per-trial values.

Report sample counts, individual values, median and observed maximum for each
condition, plus paired/local-block deltas. Do not claim p95/p99 from this screen.
Promote an effect from tentative to supported only after repeated controls
and consistent phase-level evidence.

## Reproduction

Use the project's Android Nix environment and ARM64 only. The working sources
and generated library must correspond to the normal release APK before building.
The recorder requires adb and Perfetto's trace_processor_shell. Set
GOVUE_TRACE_PROCESSOR to the latter's executable path, or place it at
.cache/trace_processor_shell.

    node scripts/startup-ablation.ts plan
    node scripts/startup-ablation.ts build no-streaming

The builder saves a source snapshot, reconstructs the capped study reference,
applies only the named profile, builds an
ARM64 APK and records source/library/APK hashes. It restores source files and
the normal release artifacts after each build. It does not promote any APK.
The cap reconstruction restores the exact original UI source hash, so the
historical comparisons remain reproducible after production cap removal.
APKs and logs are under
.cache/startup-investigation/ablation/build/. It never installs a build, resets
app data, changes networking, or persists media addresses.

After an interrupted build:

    node scripts/startup-ablation.ts restore

Recovery refuses to overwrite unexpected concurrent source edits and retains
the backups. Run matched device blocks with the checked-in harness:

    GOVUE_DEVICE_SERIAL=<serial> node scripts/run-startup-ablation.mjs <block-name> <profile> 4 0
    GOVUE_DEVICE_SERIAL=<serial> node scripts/run-startup-ablation.mjs <warm-block-name> <profile> 0 2
    GOVUE_WARM_CACHE=uncached GOVUE_DEVICE_SERIAL=<serial> node scripts/run-startup-ablation.mjs <new-target-block> <profile> 0 2
    node scripts/summarize-startup-ablation.mjs

The first command records four cold measurements plus a discarded primer. The
second records two independently primed cached-target measurements on each of
Sintel and Tears. The runner installs the profile, preserves app data, verifies
the installed APK hash and actual resume position, and restores the production
APK when available (otherwise the full reference) when finished. Use
GOVUE_RESTORE_PROFILE to choose explicitly. Keep the phone free of other use. Seek input is refused when
GoVue is not in the foreground; a trial ending in another app is excluded.
Results and block metadata are under .cache/startup-investigation/ablation/.
The summary reads individual JSON files so exclusions survive reanalysis; it
does not treat the append-only JSONL log as a deduplicated dataset.
Use GOVUE_BUNNY_START=0 for a separate zero-position cohort. The only calibrated
Bunny positions supported by the seek-reset harness are zero and 6:12.

For individual app, official YouTube or Brave traces use
scripts/profile-video-startup.mjs. Brave measurements use ADB and Perfetto;
they do not connect to browser DevTools. Browser frame markers have weaker
source attribution than the instrumented app, so requested position and
visible playback need separate verification. Successful builds alone are not
measurements.

## Validation and release artifacts

`npm run check` passed: both TypeScript projects, 130 JavaScript tests, Rust
formatting, Clippy and 102 Rust tests. `npx expo install --check` reported
dependencies up to date. ARM64 release APKs built successfully, including the
promoted release. The patch script was checked for idempotence. Device traces
are separate from these fixture and build checks.

The normal release APK and installed app match the cap-disabled experiment
binary. Temporary variant sources were restored; no experiment manifest is
active. The phone is left with the promoted production build, stopped/paused.
The study artifacts are ignored under .cache/startup-investigation/ablation/:
APK/source hashes, block metadata, per-trial JSON, launch clocks, Perfetto
traces, phase CSVs and screenshots. The checked-in summary tool reads individual
records, separates startup failures from exclusions, and never replaces a
missing frame with the recording-window duration.

The runner normally stops at its first invalid trial. Optional
GOVUE_COUNT_MEDIA_FAILURES=1 permits counting subsequent cold measurements
after a selected media format receives CDN 403s with no frame. Failed attempts
retain only the requested position, since no actual first-frame position is
observable. This option still stops on extraction verification failures, APK
mismatches, foreground changes or an incorrect observed resume position.
