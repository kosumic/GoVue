import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const directory = resolve(process.argv[2] ?? '.cache/startup-investigation/ablation');
const records = readdirSync(directory).filter(name => name.endsWith('.json') &&
  existsSync(resolve(directory, name.slice(0, -5) + '-launch.txt'))).flatMap(name => {
  const record = JSON.parse(readFileSync(resolve(directory, name), 'utf8'));
  record.measurementOutcome ??= record.valid ? 'ok' : record.firstFrameMs === null && record.foregroundAtEnd === true &&
    (!record.expectedApkSha256 || record.expectedApkSha256 === record.installedApkSha256) ? 'startup-failure' : 'excluded';
  if (record.role === 'measurement' && record.firstFiveSecondsPlayingMs === undefined) {
    const events = (record.phases ?? []).filter(p => /^GoVue playback: playing (true|false)$/.test(p.name) && p.atMs >= (record.nativePrepareMs ?? 0));
    record.firstFiveSecondsPlayingMs = events.find((p, i) => p.name.endsWith('true') &&
      (events.slice(i + 1).find(next => next.name.endsWith('false'))?.atMs ?? record.traceWindowSeconds * 1000) - p.atMs >= 5000)?.atMs ?? null;
  }
  return record.role === 'measurement' ? [record] : [];
});
const median = values => {
  const sorted = values.filter(Number.isFinite).sort((a, b) => a - b), middle = Math.floor(sorted.length / 2);
  return !sorted.length ? null : sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const seconds = value => value === null ? '—' : (value / 1000).toFixed(3);
const groups = new Map();
for (const row of records) {
  if (row.ablationEligible === false || row.measurementOutcome === 'excluded') continue;
  const key = [row.block ?? row.variant, row.video, row.mode, row.cachedExtraction ? 'cached' : 'uncached'].join('/');
  if (!groups.has(key)) groups.set(key, []);
  groups.get(key).push(row);
}
const summaries = [...groups].map(([key, rows]) => {
  rows.sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
  const metric = field => { const values = rows.map(r => r[field]).filter(Number.isFinite); return { medianMs: median(values), maxMs: values.length ? Math.max(...values) : null }; };
  const temperatures = rows.flatMap(r => [r.deviceBefore, r.deviceAfter]).filter(Boolean).map(c => c.temperatureTenthsC).filter(Number.isFinite);
  return { key, variant: rows[0].variant, block: rows[0].block, video: rows[0].video, mode: rows[0].mode,
    cached: rows[0].cachedExtraction, n: rows.filter(r => r.valid).length, attempts: rows.length,
    startupFailures: rows.filter(r => r.measurementOutcome === 'startup-failure').length,
    firstFrame: metric('firstFrameMs'), firstPlaying: metric('firstPlayingMs'),
    firstFiveSecondsPlaying: metric('firstFiveSecondsPlayingMs'), observedFiveSecondsPlaying: rows.filter(r => r.firstFiveSecondsPlayingMs !== null).length,
    extractionMs: median(rows.map(r => r.extractionMs)), dispatchToExtractionMs: median(rows.map(r => r.extractStartMs)),
    nativeToFirstFrameMs: median(rows.map(r => r.nativeToFirstFrameMs)),
    postStartStallMs: median(rows.filter(r => r.valid && r.firstPlayingMs !== null).map(r => (r.completePostStartBufferMs ?? []).reduce((a, b) => a + b, 0))),
    stalledTrials: rows.filter(r => r.completePostStartBufferMs?.length || r.unfinishedPostStartBuffer).length,
    unfinishedStalls: rows.filter(r => r.unfinishedPostStartBuffer).length,
    temperaturesC: temperatures.length ? [Math.min(...temperatures) / 10, Math.max(...temperatures) / 10] : null,
    thermalStatuses: [...new Set(rows.flatMap(r => [r.deviceBefore?.thermalStatus, r.deviceAfter?.thermalStatus]).filter(Number.isFinite))],
    heights: [...new Set(rows.map(r => r.firstHeight))], mediaProtocols: [...new Set(rows.flatMap(r => r.mediaProtocols ?? []))],
    names: rows.map(r => r.name), frameValuesMs: rows.map(r => r.firstFrameMs), playingValuesMs: rows.map(r => r.firstPlayingMs) };
}).sort((a, b) => a.key.localeCompare(b.key));
const exclusions = records.filter(r => r.measurementOutcome === 'excluded' || r.ablationEligible === false).map(r => ({ name: r.name, valid: r.valid,
  reason: r.ablationExclusionReason ?? r.exclusionReason }));
const failures = records.filter(r => r.measurementOutcome === 'startup-failure' && r.ablationEligible !== false).map(r => ({ name: r.name,
  traceWindowSeconds: r.traceWindowSeconds, httpStatuses: [...new Set(r.httpStatuses ?? [])] }));
writeFileSync(resolve(directory, 'summary.json'), JSON.stringify({ summaries, failures, exclusions }, null, 2));
const lines = ['| Block | Variant | Video / mode | Started / attempted | Frame median / max (s) | Playing median / max (s) | First 5s playing median (s) | Stall median (s) | °C / thermal |',
  '| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |', ...summaries.map(s =>
    `| ${s.block} | ${s.variant} | ${s.video} / ${s.mode}${s.cached ? ' cached' : ''} | ${s.n} / ${s.attempts} | ${seconds(s.firstFrame.medianMs)} / ${seconds(s.firstFrame.maxMs)} | ${seconds(s.firstPlaying.medianMs)} / ${seconds(s.firstPlaying.maxMs)} | ${seconds(s.firstFiveSecondsPlaying.medianMs)} | ${seconds(s.postStartStallMs)} | ${s.temperaturesC?.join('–') ?? 'block only'} / ${s.thermalStatuses.join(',')} |`)];
writeFileSync(resolve(directory, 'summary.md'), lines.join('\n') + '\n');
console.log(lines.join('\n'));
console.log(`Startup failures: ${failures.length}; excluded measurements: ${exclusions.length}. Primers are omitted. Input formats do not prove displayed quality.`);
