# Core request lock profiling

`REQUEST_LOCK` serializes storage mutations and most `execute()` operations,
including vocabulary read-modify-write updates and transcript persistence from
extraction workers. Transcript restoration takes a short snapshot under this
lock, then reads, decodes, normalizes, and builds its response outside it. Reader
session snapshots use their existing reader mutex without taking the global lock.

Every acquisition now emits three Android Perfetto slices:

- `GoVue core request: <caller>` identifies the dispatch or background worker.
- `GoVue core: lock_wait` measures the blocking mutex acquisition.
- `GoVue core: lock_hold` measures the scope holding the acquired mutex.

The wait and hold slices are siblings inside the caller slice. Dispatch adds a
`GoVue core op: <family>` child inside the hold slice, including separate
`transcript_accept`, `transcript_restore`, `player_settings`, and `downloads`
labels. Request JSON parsing and response-envelope serialization are outside the
lock. Error returns and unwinding close slices
through RAII. A poisoned-lock failure has a wait slice without a hold slice.

Labels are static and contain no request payloads, vocabulary, caption text,
storage paths, signed URLs, or tokens. The existing NDK tracing adapter only
records when Android tracing is enabled for the app; it is a no-op on other
platforms. No timing data is persisted by the core.

## Capture and inspect

Use the diagnostic arm64 release build and recording commands in
[the transcript profiling instructions](legacy-features.md#verification-and-builds).
While recording, load or refresh a large transcript and use browser state,
player settings, vocabulary, and downloads. Include warm cache restoration and
background extraction/AI work in separate recordings when investigating them.

Open the recording in [Perfetto](https://ui.perfetto.dev) and search for
`GoVue core request:`. Export one row per completed acquisition with:

```sh
trace_processor_shell query -f scripts/core-lock-profile.sql .cache/transcript.pftrace
```

The table pairs slices by their direct parent IDs rather than overlapping time
windows, so simultaneous workers do not get attributed to each other. It shows
wait/hold durations and their start times, caller, operation family, process, and
native thread. Incomplete acquisitions at recording boundaries are excluded; a
missing hold is reported as null. Use the timeline to distinguish a poisoned
acquisition from a recording that began or ended between its child slices.

Compare repeated samples and the longest waits with other workers' overlapping
hold slices in the same process. Small transcript timings alone do not establish
that a lock split will improve responsiveness. Narrow a measured bottleneck only
after auditing the state and storage it shares with dispatch and workers, and
preserve atomic storage updates and lock ordering. Verify any split live on
Android and check concurrent vocabulary updates and transcript cache access.

## Pixel measurements, 2026-10-22

The measurements in this section precede the snapshot optimization.

The instrumented arm64 release was built and installed on a Pixel 9 Pro. A
temporary instrumentation helper called the installed release's UniFFI boundary
with generated caption documents in a separate native cache directory. It ran
five transcript calls per case alongside four workers reading browser state,
player settings, vocabulary, and the download list. Each small-request worker
paused 10 ms between calls. All calls succeeded; the helper deleted its fixture
cache afterward. The Perfetto recording reported no data-loss errors.

| Transcript operation | Lines | Maximum lock hold (ms) | Maximum wait among the four small-request workers (ms) |
| --- | ---: | ---: | ---: |
| Accept | 500 | 11.187 | 10.953 |
| File restore | 500 | 8.515 | 6.094 |
| Memory restore | 500 | 1.616 | 4.603 |
| Accept | 5,000 | 83.665 | 78.251 |
| File restore | 5,000 | 92.916 | 92.566 |
| Memory restore | 5,000 | 19.272 | 17.683 |
| Accept | 20,000 | 289.837 | 286.510 |
| File restore | 20,000 | 216.251 | 214.870 |
| Memory restore | 20,000 | 92.661 | 92.310 |

File restoration used an omitted `trackId`, which reads and normalizes the
cached file; the operating system's file cache was warm. Memory restoration
specified the already cached track. These are bounded generated workloads,
not measurements of typical interaction frequency or cold flash storage.
The 5,000-line accept request was 528,806 bytes; the 20,000-line request was
2,143,806 bytes, both within the existing request and document limits.

The largest small-request waits overlapped transcript hold slices in the same
process. For example, a 284.076 ms player-settings wait overlapped 282.633 ms of
a 289.837 ms transcript-accept hold. Small requests generally held the mutex for
well under a millisecond themselves. This confirms that large transcript work
can delay unrelated requests through this lock.

In the 5,000-line accept case, average hold time was 77.372 ms: reply parsing,
transcript decoding, and normalization accounted for 15.400, 15.333, and
25.506 ms respectively, while the cache-save slice averaged 4.439 ms. This
supports investigating pure transcript preparation outside the shared lock
before changing storage synchronization. File restoration also needs attention;
vocabulary read-modify-write operations still require serialization. The normal
standalone extractor already parses and normalizes captions before taking its
persistence lock; the accept measurements above exercise the JSON `accept`
boundary, rather than the normal network extraction pipeline.

A separate UI capture opened the public `rfscVS0vtbw` video, loaded its English
transcript, and refreshed it successfully. Reader search also returned matches;
the caption-details dialog confirmed 2,935 source cues. In the uninterrupted
120-second capture, the largest lock wait was 8.674 ms (vocabulary). The largest
transcript-session hold was 48.073 ms, and the extractor's transcript-persistence
hold was 39.208 ms. This sample did not reproduce the long waits from the
generated workload. The UI recording also reported no data-loss errors.

Local evidence is in `.cache/request-lock-pixel/`: `stress.pftrace`,
`stress-acquisitions.csv` from the checked-in query, `stress-scenarios.csv`,
`stress-stages.csv`, `stress-blockers.csv`, sanitized helper results in
`stress-results.txt`, and the APK/library hashes in `build-sha256.txt`. The normal
workflow is recorded in `ui-verified.pftrace`, with `ui-summary.csv`,
`ui-integrity.csv`, and screenshots confirming the reader, refresh, search, and
caption metadata. The temporary helper APK was uninstalled after measurement;
the profiling release remains installed.

## Snapshot synchronization

Warm transcript reads retain an immutable `Arc<Transcript>` from the bounded
three-document memory cache. They no longer clone every cue, and callbacks run
after releasing the memory-cache mutex. Extraction also builds its transcript
response after releasing the persistence lock. Cache encoding, atomic file
replacement, pruning, and vocabulary updates retain their existing global lock.

Restoration without a track still selects the twenty newest eligible cache
files, preserving audio-track matching and the previous fallback order. It opens
the files while holding `REQUEST_LOCK`; these handles retain the selected file
versions across atomic replacement or eviction. Decoding reads one document at
a time, capped at the existing document byte limit. An explicit-track restore
uses the matching immutable memory document or an opened file for that identity.

Each memory-cache mutation replaces a revision token. A decoded file snapshot
enters the memory cache only if its token is still current. A concurrent accept,
extraction, reader seed, or restore therefore prevents older work from replacing
newer cached data. The caller can still receive its consistent earlier snapshot;
reader cancellation and revision checks continue to govern its presentation.
Invalidation is conservative across all cache roots and may skip a cache insert
after an unrelated cache change.

## Pixel comparison after the snapshot change

The same Pixel 9 Pro ran the same helper against the optimized arm64 release:
five transcript calls per case, with the same four small-request workers and
fixture sizes. All nine cases succeeded. The retained 30-second recording
contains all 45 transcript acquisitions and reports no trace loss or buffer
overwrites. A longer preliminary recording overwrote its earliest scenarios;
it is excluded from the comparison below.

All durations below are milliseconds. Small-request wait is the maximum across
browser state, player settings, vocabulary, and downloads in that scenario.

| Restore | Cues | Max hold before | Max hold after | Max small-request wait before | Max small-request wait after |
| --- | ---: | ---: | ---: | ---: | ---: |
| File | 500 | 8.515 | 0.572 | 6.094 | 2.345 |
| Memory | 500 | 1.616 | 0.099 | 4.603 | 1.696 |
| File | 5,000 | 92.916 | 0.251 | 92.566 | 2.052 |
| Memory | 5,000 | 19.272 | 0.042 | 17.683 | 1.710 |
| File | 20,000 | 216.251 | 0.286 | 214.870 | 1.258 |
| Memory | 20,000 | 92.661 | 0.067 | 92.310 | 3.394 |

These samples demonstrate shorter lock holds and waits under the generated
restoration workload; they do not establish a typical user-visible speedup.
The legacy JSON accept path still parses and normalizes inside its dispatch
lock: its 20,000-cue maximum hold was 146.405 ms in the new recording. Cache
encoding and persistence also remain serialized. Further narrowing needs its
own measurements and synchronization audit.

`npm run check` passed with 156 JavaScript tests and 136 Rust tests, including
the new replacement-snapshot and concurrent vocabulary/cache regression cases.
`npx expo install --check` also passed. Host checks used RAM-backed temporary
fixtures because of the previously observed host filesystem delays.

The reader also restored the saved public `rfscVS0vtbw` transcript, refreshed it,
and found 265 matches for `function`. Caption details confirmed all 2,935 source
cues. The temporary profiling helper was uninstalled afterward; the optimized
profiling release remains installed.

Local evidence is in `.cache/transcript-snapshot-pixel/`: the retained capture
is `verified-stress.pftrace`, with `verified-stress-acquisitions.csv`,
`verified-stress-scenarios.csv`, `verified-stress-stages.csv`,
`verified-stress-integrity.csv`, `verified-stress-results.txt`, and APK/library
hashes in `build-sha256.txt`. Screenshots `reader-ui.png`, `refreshed-ui.png`,
`search-ui.png`, and `details-ui.png` record the reader verification.
