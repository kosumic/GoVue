import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { setImmediate } from 'node:timers/promises';
import { build } from 'esbuild';

const require = createRequire(import.meta.url);
const transcript = { videoId: 'abcdefghijk', trackId: '.en', title: 'Video', language: 'English' };

async function harness(t) {
  const nativeEvents = new Map(), appEvents = new Set(), starts = [], coreCalls = [], errors = [], writes = [];
  const appState = { currentState: 'active', addEventListener(_name, listener) { appEvents.add(listener); return { remove: () => appEvents.delete(listener) }; } };
  const platform = { OS: 'android' };
  const bundle = await build({ entryPoints: ['src/ai/voiceInput.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'voice-core', setup(builder) {
      builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'test-core', external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name => {
    if (name === 'react-native') return { AppState: appState, Platform: platform };
    if (name === 'test-core') return {};
    return require(name);
  }, module, module.exports);
  let phase = 'idle', draft = 'Original', resultReply = {}, emptyEnd = false, nativeState = 'inactive', aborts = 0, stops = 0;
  function emit(name, value) {
    if (name === 'end') nativeState = 'inactive';
    if (name === 'start') nativeState = 'recognizing';
    for (const listener of [...nativeEvents.get(name) ?? []]) listener(value);
  }
  const recognizer = {
    addListener(name, listener) {
      const listeners = nativeEvents.get(name) ?? new Set();
      nativeEvents.set(name, listeners); listeners.add(listener);
      return { remove: () => listeners.delete(listener) };
    },
    isRecognitionAvailable: () => true,
    requestPermissionsAsync: async () => ({ granted: true, canAskAgain: true }),
    getStateAsync: async () => nativeState,
    start(options) { starts.push(options); nativeState = 'starting'; },
    stop() { stops++; nativeState = 'stopping'; },
    abort() { aborts++; nativeState = 'stopping'; },
  };
  const deps = {
    load: async () => recognizer,
    // This fake supplies protocol replies; the Rust suite tests state transitions,
    // joining, validation and limits through the real exported JSON boundary.
    core(command) {
      coreCalls.push(command);
      if (command.op === 'hints') return ['draft vocabulary'];
      const phases = { begin: 'preparing', listening: 'listening', finish: 'finishing', cancel: 'finishing', end: 'idle' };
      if (phases[command.op]) phase = phases[command.op];
      return { phase, interimText: '', ...(command.op === 'result' ? resultReply : {}), ...(command.op === 'end' ? { noSpeech: emptyEnd } : {}) };
    },
    hints: async () => ['chat vocabulary', 'caption vocabulary'], locale: () => 'en-US',
  };
  const controller = new module.exports.VoiceInput(deps);
  const request = { owner: 'chat-one', chatId: 'saved-chat', transcript, draft,
    readDraft: () => draft, onDraft: value => { draft = value; writes.push(value); }, onError: (message, settings) => errors.push({ message, settings }) };
  t.after(() => { const owner = controller.getSnapshot().owner; if (owner) controller.cancel(owner); emit('end'); });
  return { controller, request, recognizer, deps, platform, starts, coreCalls, errors, writes, nativeEvents, appEvents, emit,
    draft: () => draft, edit: value => { draft = value; }, result: reply => { resultReply = reply; }, empty: () => { emptyEnd = true; },
    aborts: () => aborts, stops: () => stops,
    app(state) { appState.currentState = state; for (const listener of [...appEvents]) listener(state); },
  };
}

test('speech previews stay outside the draft and final results use its latest edit', async t => {
  const env = await harness(t);
  await env.controller.start(env.request);
  assert.deepEqual(env.starts[0].contextualStrings, ['chat vocabulary', 'caption vocabulary']);
  assert.equal(env.starts[0].recordingOptions.persist, false);
  env.emit('start');
  env.result({ interimText: 'unconfirmed' });
  env.emit('result', { isFinal: false, results: [{ transcript: 'unconfirmed' }] });
  assert.equal(env.controller.getSnapshot().interimText, 'unconfirmed');
  assert.equal(env.draft(), 'Original');
  assert.deepEqual(env.writes, []);
  env.edit('Edited while listening');
  env.result({ draft: 'Edited while listening confirmed' });
  env.emit('result', { isFinal: true, results: [{ transcript: 'confirmed' }] });
  assert.equal(env.coreCalls.at(-1).draft, 'Edited while listening');
  assert.deepEqual(env.writes, ['Edited while listening confirmed']);
  env.emit('end');
  assert.equal(env.controller.getSnapshot().phase, 'idle');
  assert.deepEqual(env.errors, []);
});

test('stop accepts the pending final result before releasing the microphone', async t => {
  const env = await harness(t);
  await env.controller.start(env.request); env.emit('start');
  env.controller.stop(env.request.owner);
  assert.equal(env.stops(), 1);
  assert.equal(env.controller.getSnapshot().phase, 'finishing');
  env.result({ draft: 'Original last words' });
  env.emit('result', { isFinal: true, results: [{ transcript: 'last words' }] });
  assert.equal(env.draft(), 'Original last words');
  assert.equal(env.controller.getSnapshot().phase, 'finishing');
  env.emit('end');
  assert.equal(env.appEvents.size, 0);
});

test('cancel drains old native callbacks before another chat can own the microphone', async t => {
  const env = await harness(t);
  await env.controller.start(env.request); env.emit('start');
  const oldResult = [...env.nativeEvents.get('result')][0], oldEnd = [...env.nativeEvents.get('end')][0];
  env.controller.cancel(env.request.owner);
  assert.equal(env.aborts(), 1);
  assert.equal(env.controller.getSnapshot().phase, 'finishing');
  env.result({ draft: 'Should never appear' });
  oldResult({ isFinal: true, results: [{ transcript: 'late' }] });
  await env.controller.start({ ...env.request, owner: 'chat-two' });
  assert.equal(env.starts.length, 1);
  assert.deepEqual(env.writes, []);
  env.emit('end');
  await env.controller.start({ ...env.request, owner: 'chat-two' }); env.emit('start');
  oldResult({ isFinal: true, results: [{ transcript: 'late' }] }); oldEnd();
  assert.equal(env.controller.getSnapshot().owner, 'chat-two');
  assert.equal(env.starts.length, 2);
  assert.deepEqual(env.writes, []);
});

test('a permission dialog may background the app and resume after the grant', async t => {
  const env = await harness(t);
  let grant;
  env.recognizer.requestPermissionsAsync = () => new Promise(resolve => { grant = resolve; });
  const pending = env.controller.start(env.request);
  await setImmediate();
  env.app('background');
  assert.equal(env.aborts(), 0);
  grant({ granted: true }); await setImmediate();
  assert.equal(env.starts.length, 0);
  env.app('active'); await pending;
  assert.equal(env.starts.length, 1);
  assert.deepEqual(env.errors, []);
});

test('cancelling permission preparation ignores a late grant', async t => {
  const env = await harness(t);
  let grant;
  env.recognizer.requestPermissionsAsync = () => new Promise(resolve => { grant = resolve; });
  const pending = env.controller.start(env.request);
  await setImmediate();
  env.controller.cancel(env.request.owner);
  grant({ granted: true }); await pending;
  assert.equal(env.starts.length, 0);
  assert.equal(env.controller.getSnapshot().phase, 'idle');
  assert.equal(env.appEvents.size, 0);
});

test('a stalled context lookup starts recognition with draft hints after the deadline', { timeout: 10000 }, async t => {
  const env = await harness(t);
  let complete;
  env.deps.hints = () => new Promise(resolve => { complete = resolve; });
  await env.controller.start(env.request);
  assert.deepEqual(env.starts[0].contextualStrings, ['draft vocabulary']);
  complete(['late context']); await setImmediate();
  assert.equal(env.starts.length, 1);
  assert.deepEqual(env.starts[0].contextualStrings, ['draft vocabulary']);
});

test('permission denial reports Settings access without recording or changing the draft', async t => {
  const env = await harness(t);
  env.recognizer.requestPermissionsAsync = async () => ({ granted: false, canAskAgain: false });
  await env.controller.start(env.request);
  assert.equal(env.errors.length, 1);
  assert.equal(env.errors[0].settings, true);
  assert.equal(env.starts.length, 0);
  assert.deepEqual(env.writes, []);
  assert.equal(env.controller.getSnapshot().phase, 'idle');
});

test('native failure and empty end report feedback while discarding partial speech', async t => {
  const env = await harness(t);
  await env.controller.start(env.request); env.emit('start');
  env.result({ interimText: 'uncertain' });
  env.emit('result', { isFinal: false, results: [{ transcript: 'uncertain' }] });
  env.emit('error', { error: 'network' });
  assert.match(env.errors[0].message, /connection/);
  assert.equal(env.controller.getSnapshot().phase, 'finishing');
  assert.equal(env.controller.getSnapshot().interimText, '');
  assert.deepEqual(env.writes, []);
  env.emit('end');
  assert.equal(env.controller.getSnapshot().phase, 'idle');
  await env.controller.start(env.request); env.emit('start');
  env.empty(); env.emit('end');
  assert.match(env.errors.at(-1).message, /No speech/);
  assert.equal(env.starts[0].continuous, false);
});

test('backgrounding a recording aborts it and iOS supports continuous dictation', async t => {
  const env = await harness(t);
  env.platform.OS = 'ios';
  await env.controller.start(env.request); env.emit('start');
  assert.equal(env.starts[0].continuous, true);
  env.app('background');
  env.result({ draft: 'Late speech' });
  env.emit('result', { isFinal: true, results: [{ transcript: 'late' }] });
  assert.equal(env.aborts(), 1);
  assert.deepEqual(env.writes, []);
  env.emit('end');
  assert.equal(env.controller.getSnapshot().phase, 'idle');
});
