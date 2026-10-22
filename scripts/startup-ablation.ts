import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const study = join(root, '.cache/startup-investigation/ablation/build');
const native = 'modules/govue-core/android/src/main/java/io/github/kaminarios/govue/coremodule/';
const paths = {
  app: 'App.tsx', core: native + 'GoVueCoreModule.kt', engine: native + 'NativeMediaEngine.kt',
  extractionHttp: native + 'NativeExtractionHttp.kt', ui: 'src/media/useMediaBrowser.ts',
  playback: 'core/src/media/playback.rs', transport: 'core/src/transport.rs',
  playerCache: 'core/src/extraction/player_cache.rs',
};
const library = 'modules/govue-core/android/src/main/jniLibs/arm64-v8a/libgovue_core.so';
const apk = 'android/app/build/outputs/apk/release/app-release.apk';
// Pin the pre-optimization adapter; HEAD changes after the study is committed.
const legacyRevision = '3968c3c32665c97322b6716bff2bb68a904fd4e8';
const profiles: Record<string, string[]> = {
  // "full" is the capped study reference, even after promotion of cap removal.
  full: [],
  production: ['cap'],
  'no-streaming': ['buffered'],
  'streaming-32k': ['range32'],
  'buffered-1m': ['buffered', 'range1m'],
  'no-early': ['early'],
  'no-ladder': ['ladder'],
  'no-startup-cap': ['cap'],
  'no-auto': ['ladder', 'cap'],
  'default-buffer': ['buffer'],
  'no-http3': ['quic'],
  'no-http3-media': ['quicMedia'],
  'no-http3-extraction': ['quicExtraction'],
  'okhttp': ['okhttp'],
  'prefix-128k': ['prefix'],
  'no-capability-cache': ['capabilities'],
};
const usage = 'node scripts/startup-ablation.ts plan | build <profile> | restore';
const [command = 'plan', profile = 'full'] = process.argv.slice(2);
if (!['plan', 'build', 'restore'].includes(command) || !Object.hasOwn(profiles, profile)) throw Error(usage);
const sha = (bytes: string | Buffer) => createHash('sha256').update(bytes).digest('hex');
const fileSha = (path: string) => sha(readFileSync(join(root, path)));
type Active = { profile: string; files: { path: string; backup: string; before: string; after: string }[] };
const activePath = join(study, 'active.json');

function restore() {
  if (!existsSync(activePath)) return;
  const active = JSON.parse(readFileSync(activePath, 'utf8')) as Active;
  const conflicts: string[] = [];
  for (const file of active.files) {
    if (fileSha(file.path) === file.before) continue;
    if (fileSha(file.path) !== file.after) { conflicts.push(file.path); continue; }
    copyFileSync(join(study, file.backup), join(root, file.path));
  }
  // Generated artifacts are restored independently of the temporary sources.
  copyFileSync(join(study, 'full-library.so'), join(root, library));
  copyFileSync(join(study, 'full.apk'), join(root, apk));
  if (conflicts.length) throw Error('Concurrent edits preserved in ' + conflicts.join(', ') + '; backups: ' + study);
  unlinkSync(activePath);
}

if (command === 'plan') {
  console.log(JSON.stringify({ profiles, note: 'Full is the historical capped study reference; production disables the cap. Sources and normal build artifacts are restored after every build.' }, null, 2));
} else if (command === 'restore') {
  restore();
} else {
  if (!process.env.ANDROID_HOME) throw Error('Use the project Nix Android environment.');
  if (existsSync(activePath)) throw Error('An interrupted build needs recovery: ' + usage.replace('plan | build <profile> | restore', 'restore'));
  mkdirSync(study, { recursive: true });
  mkdirSync(join(study, 'backups'), { recursive: true });
  mkdirSync(join(study, 'apks'), { recursive: true });
  mkdirSync(join(study, 'libraries'), { recursive: true });
  const common = ['scripts/patch-video.ts', 'node_modules/expo-video/android/src/main/java/expo/modules/video/player/VideoPlayer.kt'];
  const sources = new Map([...Object.values(paths), ...common].map(path => [path, readFileSync(join(root, path), 'utf8')]));
  const changed = new Map(sources);
  // Reconstruct the historical capped reference from production's uncapped UI.
  // Keep this experiment-only; normal release builds use the working sources.
  const capField = '...(prepared.startupHeight ? { govueStartupHeight: prepared.startupHeight } : {}),';
  const ui = changed.get(paths.ui)!;
  if (!ui.includes(capField)) {
    const anchor = /^([ \t]*)headers: prepared\.headers,\n(?:[ \t]*\n)?/gm;
    if ([...ui.matchAll(anchor)].length !== 2) throw Error('Startup-cap reference anchors changed.');
    changed.set(paths.ui, ui.replace(anchor, (_match, indent: string) => `${indent}headers: prepared.headers,\n${indent}${capField}\n`));
  }
  const referenceSources = new Map(changed);
  const patch = (path: string, before: string, after: string, count = 1) => {
    const text = changed.get(path)!;
    if (text.split(before).length - 1 !== count) throw Error('Unexpected source for ' + path + ': ' + before.slice(0, 70));
    changed.set(path, text.split(before).join(after));
  };
  const replaceSection = (path: string, start: string, end: string, replacement: string) => {
    const text = changed.get(path)!;
    const begin = text.indexOf(start), finish = text.indexOf(end, begin + start.length);
    if (begin < 0 || finish < 0 || text.indexOf(start, begin + start.length) >= 0) throw Error('Unexpected source section: ' + path);
    changed.set(path, text.slice(0, begin) + replacement + text.slice(finish));
  };
  for (const factor of profiles[profile]) {
    if (factor === 'buffered') patch(paths.core, '      installMediaStreamHttp(mediaHttp)\n', '');
    else if (factor === 'range32') patch(paths.transport, '        chunk: if streaming::available() {\n            CHUNK\n        } else {\n            INITIAL_CHUNK\n        },', '        chunk: INITIAL_CHUNK,');
    else if (factor === 'range1m') patch(paths.transport, '        chunk: if streaming::available() {\n            CHUNK\n        } else {\n            INITIAL_CHUNK\n        },', '        chunk: CHUNK,');
    else if (factor === 'early') {
      replaceSection(paths.app, '      if (next.target && youtubeVideoId(next.target.url)', '      if (next.target?.load)', '');
      patch(paths.app, '      const previousTabs = browserRef.current.tabs;\n', '');
      replaceSection(paths.core, '    // Rust validates the OS-delivered text and settings, then starts a bounded\n', '  private var pendingSave:', '  }\n');
    } else if (factor === 'ladder') {
      replaceSection(paths.playback, "pub(crate) fn adaptive_videos<'a>", 'fn video_formats(',
        "pub(crate) fn adaptive_videos<'a>(catalog: &'a Value, _selected: &Value) -> Result<Vec<&'a Value>> {\n    Ok(videos(catalog)?.into_iter().filter(|stream| stream[\"kind\"] == \"video\").collect())\n}\n\n");
    } else if (factor === 'cap') patch(paths.ui, '...(prepared.startupHeight ? { govueStartupHeight: prepared.startupHeight } : {}),', '', 2);
    else if (factor === 'buffer') patch(paths.ui, '    player.bufferOptions = { minBufferForPlayback: 0.25 };', "    // Ablation: use Expo's default playback buffer threshold.");
    else if (factor === 'prefix') patch(paths.playerCache, '&[], 65_536)?;', '&[], 131_072)?;');
    else if (factor === 'quic') patch(paths.engine, '.setEnableQuic(true).addQuicHint(host, 443, 443).build()', '.setEnableQuic(false).build()');
    else if (factor === 'quicMedia' || factor === 'quicExtraction') {
      patch(paths.engine, 'private val capacity: Semaphore) {', 'private val capacity: Semaphore, private val enableQuic: Boolean = true) {');
      patch(paths.engine, '.setEnableQuic(true)', '.setEnableQuic(enableQuic)');
      if (factor === 'quicMedia') patch(paths.core, 'NativeMediaEngine(context, capacity)', 'NativeMediaEngine(context, capacity, false)');
      else patch(paths.extractionHttp, 'NativeMediaEngine(context, java.util.concurrent.Semaphore(24))', 'NativeMediaEngine(context, java.util.concurrent.Semaphore(24), false)');
    } else if (factor === 'okhttp') {
      patch(paths.core, 'private val http3 = if (Build.VERSION.SDK_INT >= 34)', 'private val http3 = if (false && Build.VERSION.SDK_INT >= 34)');
      patch(paths.extractionHttp, 'private val http3 = if (Build.VERSION.SDK_INT >= 34)', 'private val http3 = if (false && Build.VERSION.SDK_INT >= 34)');
    } else if (factor === 'capabilities') {
      const old = execFileSync('git', ['show', legacyRevision + ':' + paths.core], { cwd: root, encoding: 'utf8' });
      const start = '    AsyncFunction("supportedMediaFormats")', end = '    AsyncFunction("getSystemVolume")';
      let handler = old.slice(old.indexOf(start), old.indexOf(end, old.indexOf(start)));
      if (!handler.includes('getCapabilitiesForType') || handler.includes('mediaCapabilities.supports')) throw Error('Pinned revision does not contain the original capability scan.');
      handler = handler.replace('{ raw: String ->\n', '{ raw: String ->\n      mediaTrace("GoVue playback: format support") {\n');
      const closing = handler.lastIndexOf('    }\n');
      handler = handler.slice(0, closing) + '      }\n' + handler.slice(closing);
      replaceSection(paths.core, start, end, handler);
    } else throw Error('Unknown factor: ' + factor);
  }

  const sourceFiles = execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard'], { cwd: root, encoding: 'utf8' })
    .trim().split('\n').filter(path => /^(core\/src\/|Cargo|rust-toolchain|\.cargo\/)/.test(path));
  const rustHash = () => sha(sourceFiles.map(path => path + ':' + sha(changed.get(path) ?? readFileSync(join(root, path)))).join('\n'));
  const baselineRustHash = sha(sourceFiles.map(path => path + ':' + sha(sources.get(path) ?? readFileSync(join(root, path)))).join('\n'));
  const cached = join(study, 'libraries', rustHash() + '.so');
  const fullCached = join(study, 'libraries', baselineRustHash + '.so');
  if (!existsSync(fullCached)) copyFileSync(join(root, library), fullCached);
  copyFileSync(join(root, library), join(study, 'full-library.so'));
  copyFileSync(join(root, apk), join(study, 'full.apk'));
  const active: Active = { profile, files: [] };
  for (const [path, text] of changed) {
    if (text === sources.get(path)) continue;
    const backup = 'backups/' + sha(path) + '.txt';
    writeFileSync(join(study, backup), sources.get(path)!);
    active.files.push({ path, backup, before: sha(sources.get(path)!), after: sha(text) });
  }
  writeFileSync(activePath, JSON.stringify(active, null, 2));

  const run = (name: string, args: string[], log: string) => new Promise<void>((accept, reject) => {
    const output = join(study, log);
    const child = spawn(name, args, { cwd: root, env: { ...process.env, NODE_ENV: 'production', CARGO_BUILD_JOBS: '1', CARGO_INCREMENTAL: '0' }, stdio: ['ignore', 'pipe', 'pipe'] });
    const chunks: Buffer[] = [];
    child.stdout.on('data', (b: Buffer) => chunks.push(b));
    child.stderr.on('data', (b: Buffer) => chunks.push(b));
    child.on('error', reject);
    child.on('close', code => {
      writeFileSync(output, Buffer.concat(chunks));
      if (code === 0) accept();
      else reject(Error(name + ' exited ' + code + '; log: ' + relative(root, output)));
    });
  });
  try {
    for (const [path, text] of changed) if (text !== sources.get(path)) writeFileSync(join(root, path), text);
    if (!existsSync(cached)) {
      console.log('Compiling Rust for ' + profile);
      await run('cargo', ['build', '--locked', '--release', '-p', 'govue-core', '--lib', '--target', 'aarch64-linux-android'], profile + '-rust.log');
      copyFileSync(join(root, 'target/aarch64-linux-android/release/libgovue_core.so'), cached);
    }
    copyFileSync(cached, join(root, library));
    console.log('Building ARM64 APK for ' + profile);
    await run('android/gradlew', ['-p', 'android', ':app:assembleRelease', '-PreactNativeArchitectures=arm64-v8a',
      '--max-workers=1', '--no-configuration-cache', '-Dorg.gradle.jvmargs=-Xmx3g -XX:MaxMetaspaceSize=1536m',
      '-x', ':govue-core:buildRust'], profile + '-android.log');
    for (const [path, expected] of changed) if (fileSha(path) !== sha(expected)) throw Error('Source changed during build: ' + path);
    const destination = join(study, 'apks', profile + '.apk');
    copyFileSync(join(root, apk), destination);
    const metadata = { profile, factors: profiles[profile], rustHash: rustHash(), librarySha256: sha(readFileSync(cached)), apkSha256: sha(readFileSync(destination)),
      baselineSources: Object.fromEntries([...referenceSources].map(([path, source]) => [path, sha(source)])),
      changedSources: active.files.map(file => ({ path: file.path, sha256: file.after })), builtAt: new Date().toISOString() };
    writeFileSync(join(study, 'apks', profile + '.json'), JSON.stringify(metadata, null, 2));
    console.log('Built ' + relative(root, destination) + ' (sha256 ' + metadata.apkSha256 + ')');
  } finally {
    restore();
  }
}
