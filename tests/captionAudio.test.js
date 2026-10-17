import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const scope = { chatId: 'chat', videoId: 'abcdefghijk', trackId: '.en' };
const clip = token => ({ needsMedia: false, token, uri: `http://127.0.0.1:1234/${token}/index.m3u8`, start: 2.5, end: 4.5 });
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };

async function harness(t, respond = async command => clip(command.cueId)) {
  const calls = [], extractions = [], players = [], apps = new Set();
  let transcript, game;
  const result = await build({ entryPoints: ['src/ai/useGameAudio.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'caption-edges', setup(builder) {
      builder.onResolve({ filter: /modules\/lingoplay-core$/ }, () => ({ path: 'mock-core', external: true }));
      builder.onResolve({ filter: /\/core\/media$/ }, () => ({ path: 'mock-media', external: true }));
    } }],
  });
  const core = { async aiRequest(command) { calls.push(command); return command.op.endsWith('_release') ? null : respond(command); } };
  const media = { async extractMedia(videoId, signal, fresh) { extractions.push({ videoId, signal, fresh }); return { videoId }; } };
  function useVideoPlayer(_, setup) {
    const [player] = React.useState(() => {
      const listeners = new Map();
      const player = { currentTime: 0, playing: false, source: null, sources: [],
        pause() { player.playing = false; }, play() { player.playing = true; },
        async replaceAsync(source) { player.source = source; player.sources.push(source); },
        addListener(event, listener) {
          const handlers = listeners.get(event) ?? new Set(); listeners.set(event, handlers); handlers.add(listener);
          return { remove: () => handlers.delete(listener) };
        },
        emit(event, payload) { for (const listener of [...listeners.get(event) ?? []]) listener(payload); },
      };
      setup(player); players.push(player); return player;
    });
    return player;
  }
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => {
    if (name === 'mock-core') return core;
    if (name === 'mock-media') return media;
    if (name === 'expo-video') return { useVideoPlayer };
    if (name === 'react-native') return { AppState: { addEventListener(_, listener) { apps.add(listener); return { remove: () => apps.delete(listener) }; } } };
    return require(name);
  }, module, module.exports);
  let pausedVideo = 0;
  function Audio() {
    transcript = module.exports.useTranscriptAudio(scope, () => { ++pausedVideo; });
    game = module.exports.useGameAudio({ chatId: scope.chatId, messageId: 'reply', gameId: 'game' });
    return null;
  }
  let root;
  await act(async () => { root = create(createElement(Audio)); });
  t.after(() => act(async () => root.unmount()));
  return { calls, extractions, players, apps, transcript: () => transcript, game: () => game, pausedVideo: () => pausedVideo,
    unmount: () => act(async () => root.unmount()) };
}

test('a cached transcript line plays only its caption range and releases its local route', async t => {
  const env = await harness(t);
  assert.equal(env.calls.length, 0, 'opening the reader stays lazy');
  for (let replay = 0; replay < 2; ++replay) {
    let playing;
    await act(() => { playing = env.transcript().play('cue-7'); });
    const player = env.players[0];
    assert.equal(env.transcript().phase, 'playing');
    assert.equal(player.currentTime, 2.5);
    assert.equal(player.playing, true);
    assert.equal(env.pausedVideo(), replay + 1);
    await act(() => player.emit('timeUpdate', { currentTime: 4.49 }));
    assert.equal(player.playing, true);
    await act(async () => { player.emit('timeUpdate', { currentTime: 4.5 }); await playing; });
    assert.equal(player.playing, false);
    assert.equal(player.source, null);
    assert.equal(env.transcript().phase, 'idle');
  }
  assert.equal(env.extractions.length, 0, 'cached replay requires no media extraction');
  assert.deepEqual(env.calls.filter(c => c.op === 'ai_chat_audio'), Array(2).fill({ op: 'ai_chat_audio', ...scope, cueId: 'cue-7' }));
  assert.equal(env.calls.filter(c => c.op === 'ai_game_audio_release').length, 2);
});

test('switching lines discards and revokes a late clip without interrupting the newer line', async t => {
  const old = deferred();
  const env = await harness(t, command => command.cueId === 'cue-0' ? old.promise : clip('new'));
  let first, second;
  await act(() => { first = env.transcript().play('cue-0').catch(error => error); });
  await act(() => { second = env.transcript().play('cue-1'); });
  assert.equal(env.transcript().phase, 'playing');
  await act(async () => { old.resolve(clip('old')); assert.match((await first).message, /stopped/); });
  assert.equal(env.players[0].playing, true);
  assert.equal(env.players[0].sources.some(source => source?.uri.includes('/old/')), false);
  assert.ok(env.calls.some(c => c.op === 'ai_game_audio_release' && c.token === 'old'));
  await act(async () => { env.players[0].emit('timeUpdate', { currentTime: 4.5 }); await second; });
});

test('games and transcript lines share audio focus, and backgrounding cancels playback', async t => {
  const env = await harness(t);
  let transcript, game;
  await act(() => { transcript = env.transcript().play('cue-0').catch(error => error); });
  await act(() => { game = env.game().play('cue-1').catch(error => error); });
  assert.match((await transcript).message, /stopped/);
  assert.equal(env.players[0].playing, false);
  assert.equal(env.players[1].playing, true);
  assert.equal(env.transcript().phase, 'idle');
  await act(async () => { for (const listener of env.apps) listener('background'); await game; });
  assert.equal(env.players[1].playing, false);
  assert.equal(env.game().phase, 'idle');
  assert.equal(env.calls.filter(c => c.op === 'ai_game_audio_release').length, 2);
});

test('closing the reader cancels extraction and releases a clip returned after unmount', async t => {
  const pending = deferred();
  const env = await harness(t, command => command.catalog ? pending.promise : { needsMedia: true, videoId: scope.videoId });
  let playing;
  await act(() => { playing = env.transcript().play('cue-0').catch(error => error); });
  assert.equal(env.extractions.length, 1);
  await env.unmount();
  assert.equal(env.extractions[0].signal.aborted, true);
  pending.resolve(clip('late'));
  assert.match((await playing).message, /stopped/);
  assert.equal(env.players[0].sources.length, 0);
  assert.ok(env.calls.some(c => c.op === 'ai_game_audio_release' && c.token === 'late'));
});

test('loading failures give feedback and the next tap can retry', async t => {
  let attempt = 0;
  const env = await harness(t, () => { if (++attempt === 1) throw Error('Caption audio is unavailable.'); return clip('retry'); });
  await act(async () => { await assert.rejects(env.transcript().play('cue-0'), /unavailable/); });
  assert.equal(env.transcript().phase, 'idle');
  assert.equal(env.transcript().error, 'Caption audio is unavailable.');
  let retry;
  await act(() => { retry = env.transcript().play('cue-0'); });
  assert.equal(env.transcript().error, '');
  await act(async () => { env.players[0].emit('playToEnd'); await retry; });
});
