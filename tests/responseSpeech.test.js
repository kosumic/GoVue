import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const bundle = await build({ entryPoints: ['src/ai/useExampleSpeech.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external', supported: { 'dynamic-import': false },
  plugins: [{ name: 'speech-core', setup(builder) {
    builder.onResolve({ filter: /modules\/lingoplay-core$/ }, () => ({ path: 'test-core', external: true }));
  } }],
});

async function harness(t, example = false) {
  const events = new Set(), spoken = [], plans = [];
  let controller, plan = async command => ({ chunks: [command.text] }), stops = 0, before = 0;
  const app = { currentState: 'active', addEventListener(_name, listener) { events.add(listener); return { remove: () => events.delete(listener) }; } };
  const speech = {
    maxSpeechInputLength: 6000,
    getAvailableVoicesAsync: async () => [{ identifier: 'english', language: 'en-US' }],
    stop: async () => { stops++; },
    speak: (text, options) => { spoken.push({ text, options }); },
  };
  const core = { coreRequest(command) { plans.push(command); return plan(command); } };
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name => {
    if (name === 'react-native') return { AppState: app };
    if (name === 'expo-speech') return speech;
    if (name === 'test-core') return core;
    return require(name);
  }, module, module.exports);
  function Reader({ context }) {
    controller = (example ? module.exports.useExampleSpeech : module.exports.useResponseSpeech)(context, () => { before++; });
    return null;
  }
  let root;
  await act(async () => { root = create(createElement(Reader, { context: 'chat-one' })); });
  const unmount = () => act(async () => root.unmount());
  t.after(unmount);
  return { spoken, plans, speech, reader: () => controller, stops: () => stops, before: () => before,
    plan: fn => { plan = fn; },
    update: context => act(async () => root.update(createElement(Reader, { context }))),
    app: state => act(async () => { app.currentState = state; for (const listener of events) listener(state); }),
    unmount,
  };
}

test('response speech uses Rust chunks in order, the default voice, and resets after completion', async t => {
  const env = await harness(t);
  env.plan(async () => ({ chunks: ['First sentence.', 'Second sentence.'] }));
  await act(async () => env.reader().speak('reply', '**First sentence.** Second sentence.'));
  assert.deepEqual(env.plans, [{ op: 'response_speech', text: '**First sentence.** Second sentence.', maxLength: 4000 }]);
  assert.equal(env.before(), 1);
  assert.equal(env.reader().active, 'reply');
  assert.equal(env.spoken[0].options.voice, undefined);
  assert.equal(env.spoken[0].options.language, undefined);
  assert.equal(env.spoken[0].text, 'First sentence.');
  await act(async () => env.spoken[0].options.onDone());
  assert.equal(env.spoken[1].text, 'Second sentence.');
  assert.equal(env.reader().active, 'reply');
  await act(async () => env.spoken[1].options.onDone());
  assert.equal(env.reader().active, null);
  await act(async () => env.spoken[0].options.onDone());
  assert.equal(env.spoken.length, 2, 'late completion cannot restart finished speech');
});

test('switching responses and stopping ignore callbacks from the previous utterance', async t => {
  const env = await harness(t);
  env.plan(async command => ({ chunks: [command.text, 'More text.'] }));
  await act(async () => env.reader().speak('first', 'First response.'));
  const old = env.spoken[0].options;
  await act(async () => env.reader().speak('second', 'Second response.'));
  assert.equal(env.reader().active, 'second');
  await act(async () => { old.onStopped(); old.onDone(); old.onError(); });
  assert.equal(env.spoken.length, 2);
  assert.equal(env.reader().active, 'second');
  assert.equal(env.reader().error, '');
  const stops = env.stops();
  await act(async () => env.reader().stop());
  assert.equal(env.stops(), stops + 1);
  assert.equal(env.reader().active, null);
  await act(async () => env.spoken[1].options.onDone());
  assert.equal(env.spoken.length, 2, 'stopping cancels remaining chunks');
});

test('navigation, backgrounding, and unmount cancel preparation before it can start speech', async t => {
  for (const cancel of ['navigation', 'background', 'unmount']) {
    const env = await harness(t);
    let resolve;
    env.plan(() => new Promise(done => { resolve = done; }));
    let pending;
    await act(async () => { pending = env.reader().speak('reply', 'Pending response.'); });
    assert.ok(resolve);
    if (cancel === 'navigation') await env.update('chat-two');
    else if (cancel === 'background') await env.app('background');
    else await env.unmount();
    await act(async () => { resolve({ chunks: ['Too late.'] }); await pending; });
    assert.equal(env.spoken.length, 0, cancel);
    if (cancel !== 'unmount') assert.equal(env.reader().active, null);
  }
});

test('backgrounding stops active speech and rejects stale read actions until the app is active', async t => {
  const env = await harness(t);
  await act(async () => env.reader().speak('reply', 'Response.'));
  const stops = env.stops();
  await env.app('background');
  assert.equal(env.stops(), stops + 1);
  await act(async () => env.reader().speak('stale', 'Do not read.'));
  assert.equal(env.spoken.length, 1);
  assert.equal(env.reader().active, null);
});

test('stopping during engine initialization prevents queued speech and a missing engine is reported', async t => {
  const env = await harness(t);
  let ready;
  env.speech.getAvailableVoicesAsync = () => new Promise(resolve => { ready = resolve; });
  let pending;
  await act(async () => { pending = env.reader().speak('reply', 'Response.'); });
  assert.ok(ready);
  await act(async () => env.reader().stop());
  await act(async () => { ready([{ identifier: 'english', language: 'en-US' }]); await pending; });
  assert.deepEqual(env.plans, []);
  assert.deepEqual(env.spoken, []);
  env.speech.getAvailableVoicesAsync = async () => [];
  await act(async () => env.reader().speak('unavailable', 'Response.'));
  assert.equal(env.reader().active, null);
  assert.match(env.reader().error, /No text-to-speech voice is available/);
});

test('planning and native failures reset the control and allow another response to play', async t => {
  const env = await harness(t);
  env.plan(async () => { throw Error('This response has no text to read aloud.'); });
  await act(async () => env.reader().speak('empty', '***'));
  assert.equal(env.reader().active, null);
  assert.match(env.reader().error, /no text/);
  env.plan(async command => ({ chunks: [command.text, 'Next chunk.'] }));
  await act(async () => env.reader().speak('reply', 'Response.'));
  await act(async () => env.spoken[0].options.onError());
  assert.equal(env.reader().active, null);
  assert.match(env.reader().error, /Could not read this response/);
  await act(async () => env.spoken[0].options.onDone());
  assert.equal(env.spoken.length, 1);
  await act(async () => env.reader().speak('retry', 'Try again.'));
  assert.equal(env.reader().active, 'retry');
  assert.equal(env.reader().error, '');
});

test('example speech keeps its language-specific voice and bypasses response parsing', async t => {
  const env = await harness(t, true);
  await act(async () => env.reader().speak('example', 'An example.', 'en'));
  assert.deepEqual(env.plans, []);
  assert.equal(env.spoken[0].options.voice, 'english');
  assert.equal(env.spoken[0].options.language, 'en');
  await act(async () => env.reader().speak('missing', 'Exemple.', 'fr'));
  assert.equal(env.reader().active, null);
  assert.match(env.reader().error, /No voice is available for this language/);
});
