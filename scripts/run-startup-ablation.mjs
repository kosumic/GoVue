import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const [block, variant, coldText = '4', warmText = '2'] = process.argv.slice(2);
const coldCount = Number(coldText), warmCount = Number(warmText);
const warmCache = process.env.GOVUE_WARM_CACHE ?? 'cached';
if (!['cached', 'uncached'].includes(warmCache)) throw Error('GOVUE_WARM_CACHE must be cached or uncached.');
const bunnyStart = Number(process.env.GOVUE_BUNNY_START ?? 372);
if (![0, 372].includes(bunnyStart)) throw Error('Only calibrated Bunny offsets 0 and 372 are supported.');
if (![block, variant].every(s => /^[a-z0-9-]+$/.test(s ?? '')) ||
    ![coldCount, warmCount].every(n => Number.isInteger(n) && n >= 0 && n <= 8))
  throw Error('Usage: node scripts/run-startup-ablation.mjs <block> <variant> [cold=4] [warm=2]');
const serial = process.env.GOVUE_DEVICE_SERIAL;
if (!serial) throw Error('Set GOVUE_DEVICE_SERIAL.');
const adb = process.env.ADB ?? 'adb', pkg = 'io.github.kaminarios.lingoplay';
const output = resolve(root, process.env.GOVUE_RESULTS_DIR ?? '.cache/startup-investigation/ablation');
const apks = resolve(root, '.cache/startup-investigation/ablation/build/apks');
const restoreProfile = process.env.GOVUE_RESTORE_PROFILE ?? (existsSync(resolve(apks, 'production.json')) ? 'production' : 'full');
if (!/^[a-z0-9-]+$/.test(restoreProfile)) throw Error('Invalid restore profile.');
for (const name of [variant, restoreProfile]) if (!existsSync(resolve(apks, name + '.json'))) throw Error('Build profile first: ' + name);
mkdirSync(output, { recursive: true });
const blockFile = resolve(output, block + '-block.json');
if (existsSync(blockFile)) throw Error('Block already exists; use a new name.');
const run = (...args) => execFileSync(adb, ['-s', serial, ...args], { encoding: 'utf8', timeout: 120000, maxBuffer: 5e6 });
const delay = ms => new Promise(done => setTimeout(done, ms));
const appInForeground = () => /(?:mResumedActivity|topResumedActivity)[^\n]*io\.github\.kaminarios\.lingoplay/.test(run('shell', 'dumpsys', 'activity', 'activities'));
const movies = { bunny: 'aqz-KE-bpKQ', sintel: 'eRsGyueVLvQ', tears: 'R6MlUcmOul8' };
const condition = () => {
  const battery = run('shell', 'dumpsys', 'battery'), thermal = run('shell', 'dumpsys', 'thermalservice');
  return { at: new Date().toISOString(), temperatureTenthsC: Number(battery.match(/temperature:\s*([0-9]+)/)?.[1]) || null,
    thermalStatus: Number(thermal.match(/Thermal Status:\s*([0-9]+)/i)?.[1] ?? -1) };
};
const study = { protocolVersion: 2, block, variant, restoreProfile, coldCount, warmCount, warmCache, bunnyStart, started: condition(), finished: null, completed: false, error: null, cases: [] };
writeFileSync(blockFile, JSON.stringify(study, null, 2));
async function record(name, movie, mode, seconds, role, offset, cache = 'any') {
  const caseName = block + '-' + name;
  const environment = { ...process.env, GOVUE_DEVICE_SERIAL: serial, GOVUE_VARIANT: variant, GOVUE_RESULTS_DIR: output,
    GOVUE_TRIAL_ROLE: role, GOVUE_BLOCK: block, GOVUE_PROTOCOL_VERSION: '2', GOVUE_EXPECT_CACHE: cache, GOVUE_START: String(offset ?? 0) };
  if (offset !== null) environment.GOVUE_EXPECT_POSITION = String(offset);
  else delete environment.GOVUE_EXPECT_POSITION;
  const child = spawn(process.execPath, ['scripts/profile-video-startup.mjs', caseName, pkg, movies[movie], mode, String(seconds)],
    { cwd: root, env: environment, stdio: ['ignore', 'pipe', 'pipe'] });
  let details = '';
  child.stdout.on('data', b => { details += b; });
  child.stderr.on('data', b => { details += b; });
  await new Promise((accept, reject) => {
    child.on('error', reject);
    child.on('close', code => code === 0 ? accept() : reject(Error('Recorder failed for ' + caseName + ': ' + details.slice(-1200))));
  });
  const result = JSON.parse(readFileSync(resolve(output, caseName + '.json'), 'utf8'));
  study.cases.push(caseName);
  writeFileSync(blockFile, JSON.stringify(study, null, 2));
  console.log(JSON.stringify({ event: 'case', name: caseName, role, valid: result.valid, firstFrameMs: result.firstFrameMs,
    firstPlayingMs: result.firstPlayingMs, positionMs: result.firstPositionMs, firstHeight: result.firstHeight,
    cached: result.cachedExtraction, quality10s: result.formatAtTenSeconds, protocols: result.mediaProtocols }));
  return result;
}
async function check(result) {
  if (result.valid) return true;
  if (appInForeground()) {
    const bytes = execFileSync(adb, ['-s', serial, 'exec-out', 'screencap', '-p'], { timeout: 10000, maxBuffer: 12e6 });
    writeFileSync(resolve(output, result.name + '-failure.png'), bytes);
  }
  if (process.env.GOVUE_COUNT_MEDIA_FAILURES === '1' && result.role === 'measurement' &&
      result.measurementOutcome === 'startup-failure' && result.httpStatuses.includes('403') &&
      result.phases.some(p => p.name.startsWith('GoVue playback: format '))) return false;
  throw Error(result.name + ': ' + result.exclusionReason);
}
async function resetPosition(movie, stop = false) {
  // Work outside the measurement. Keep taps in the upper part of the native
  // seek bar, above the transcript/control row. The trace checks the result.
  if (!appInForeground()) throw Error('Phone switched apps; refusing to send playback or seek input.');
  run('shell', 'input', 'keyevent', '127');
  run('shell', 'input', 'tap', '480', '530');
  await delay(650);
  if (!appInForeground()) throw Error('Phone switched apps before the seek reset.');
  run('shell', 'input', 'tap', movie === 'bunny' && bunnyStart === 372 ? '560' : '18', '660');
  await delay(6000);
  if (stop) run('shell', 'am', 'force-stop', pkg);
}
try {
  run('shell', 'am', 'force-stop', 'com.brave.browser');
  run('shell', 'am', 'force-stop', 'com.google.android.youtube');
  const installed = run('install', '-r', resolve(apks, variant + '.apk'));
  if (!installed.includes('Success')) throw Error('Variant installation did not complete.');
  console.log(JSON.stringify({ event: 'installed', variant }));
  if (coldCount) {
    await check(await record('prime-bunny', 'bunny', 'cold', 15, 'prime', null));
    let previousStarted = true;
    for (let i = 1; i <= coldCount; i++) {
      if (previousStarted) await resetPosition('bunny', true);
      previousStarted = await check(await record('bunny-cold-' + i, 'bunny', 'cold', 20, 'measurement', bunnyStart));
    }
  }
  if (warmCount) {
    if (!coldCount) await check(await record('prime-bunny-seed', 'bunny', 'cold', 15, 'prime', null));
    await resetPosition('bunny');
    for (const movie of ['sintel', 'tears']) for (let i = 1; i <= warmCount; i++) {
      // A fresh process and the same primer offsets make every warm trial
      // independent of prior trial duration and the metadata cache's age.
      await check(await record('prime-' + movie + '-' + i, movie, 'cold', 15, 'prime', null));
      await resetPosition(movie);
      await check(await record('prime-bunny-' + movie + '-' + i, 'bunny', warmCache === 'cached' ? 'warm' : 'cold', 15, 'prime', bunnyStart));
      await resetPosition('bunny');
      await check(await record(movie + '-warm-' + i, movie, 'warm', 20, 'measurement', 0, warmCache === 'cached' ? 'hit' : 'miss'));
    }
  }
  study.completed = true;
} catch (error) {
  study.error = error.message;
  throw error;
} finally {
  study.finished = condition();
  writeFileSync(blockFile, JSON.stringify(study, null, 2));
  const wasForeground = appInForeground();
  if (wasForeground) run('shell', 'input', 'keyevent', '127');
  run('shell', 'am', 'force-stop', pkg);
  if (variant !== restoreProfile) {
    const restored = run('install', '-r', resolve(apks, restoreProfile + '.apk'));
    if (!restored.includes('Success')) throw Error('Could not restore ' + restoreProfile + ' APK.');
  }
  if (wasForeground && appInForeground()) run('shell', 'input', 'keyevent', '3');
}
