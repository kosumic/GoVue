import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const tracks = [{ id: '.en', label: 'English' }, { id: '.es', label: 'Spanish' }];
const data = { videoId: 'abcdefghijk', trackId: '.en', title: 'Example', language: 'English', tracks,
  segments: [{ start: 120, duration: 2, text: 'Hello world' }, { start: 240, duration: 2, text: 'Try again' }] };
const comparison = { ...data, trackId: '.es', language: 'Spanish', segments: [{ start: 300, duration: 2, text: 'Hola mundo' }] };
const clip = (token, start = 2.5) => ({ needsMedia: false, token,
  uri: `http://127.0.0.1:1234/${token}/index.m3u8`, start, end: start + 2 });
const event = () => ({ stopPropagation() {} });

// Keep the reader, audio hook, caption panel, and word renderer connected. Only
// native playback and Rust's document/word/clip responses are supplied here.
async function harness(t, options = {}) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const calls = [], listeners = new Map(), apps = new Set();
  let player, beforeAudio = 0;
  const compiled = await build({ entryPoints: ['src/components/ChatTranscript.tsx'], bundle: true, write: false,
    platform: 'node', format: 'cjs', jsx: 'automatic', packages: 'external',
    plugins: [{ name: 'chat-transcript-edges', setup(builder) {
      builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'test-core', external: true }));
      builder.onResolve({ filter: /\/(theme|core\/transcripts|core\/vocabulary|player\/usePlayerTranscript|IconButton|WordTranslationSheet|TranscriptTranslations)$/ },
        args => ({ path: args.path, external: true }));
    } }],
  });
  const colors = new Proxy({ accent: '#f00', background: '#fff' }, { get: (value, key) => value[key] ?? '#333' });
  const native = {
    ...Object.fromEntries(['View', 'Text', 'Pressable', 'ScrollView', 'TextInput', 'Switch', 'ActivityIndicator'].map(name => [name, name])),
    StyleSheet: { create: styles => styles, hairlineWidth: 1 },
    useWindowDimensions: () => ({ fontScale: 1 }),
    AppState: { currentState: 'active', addEventListener(_, listener) { apps.add(listener); return { remove: () => apps.delete(listener) }; } },
    FlatList: props => React.createElement('FlatList', {}, props.data.map((item, index) =>
      React.createElement(React.Fragment, { key: props.keyExtractor(item, index) }, props.renderItem({ item, index })))),
  };
  const module = { exports: {} };
  Function('require', 'module', 'exports', compiled.outputFiles[0].text)(name => {
    if (name === 'react-native') return native;
    if (name === 'lucide-react-native') return new Proxy({}, { get: () => 'Icon' });
    if (name === '@react-native-clipboard/clipboard') return {};
    if (name === 'expo-video') return { useVideoPlayer: (_, setup) => {
      const [value] = React.useState(() => {
        player = { currentTime: 0, playing: false, source: null,
          pause() { player.playing = false; }, play() { player.playing = true; },
          async replaceAsync(source) { player.source = source; },
          addListener(event, listener) {
            const handlers = listeners.get(event) ?? new Set(); listeners.set(event, handlers); handlers.add(listener);
            return { remove: () => handlers.delete(listener) };
          },
        };
        setup(player); return player;
      });
      return value;
    } };
    if (name === 'test-core') return { aiRequest: async command => {
      calls.push(command);
      if (command.op !== 'ai_caption_audio') return null;
      return options.respond ? options.respond(command.command) : clip(command.command.cueId);
    } };
    if (name.endsWith('/theme')) return { useThemeColors: () => colors, useThemeStyles: factory => factory(colors) };
    if (name.endsWith('/player/usePlayerTranscript')) return { usePlayerTranscript: () => ({ data,
      comparison: options.comparison ?? null, trackIds: options.comparison ? ['.en', '.es'] : ['.en'], loading: false, error: null }) };
    if (name.endsWith('/core/transcripts')) return { readTranscripts: async () => ({
      rows: [{ start: 120, lines: [{ track: 0, index: 0 }] }, { start: 240, lines: [{ track: 0, index: 1 }] },
        ...(options.comparison ? [{ start: 300, lines: [{ track: 1, index: 0 }] }] : [])],
      total: 2, trackChoices: { sourceTracks: [], captionTracks: [], translations: [] },
    }) };
    if (name.endsWith('/core/vocabulary')) return { captionWords: async (source, index) => {
      const segment = source.segments[index];
      const [first, second] = segment.text.split(' ');
      return { text: segment.text, tokens: [
        { text: first, word: true, ...(!options.untimed && { start: segment.start, end: segment.start + 1 }) },
        { text: ' ', word: false },
        { text: second, word: true, ...(!options.untimed && { start: segment.start + 1, end: segment.start + 2 }) },
      ] };
    } };
    for (const component of ['IconButton', 'WordTranslationSheet', 'TranscriptTranslations']) {
      if (name.endsWith(`/${component}`)) return { [component]: component };
    }
    return require(name);
  }, module, module.exports);
  let root;
  await act(() => { root = create(React.createElement(module.exports.ChatTranscriptReader,
    { data, chatId: 'chat', onClose() {}, onBeforeAudio() { ++beforeAudio; } })); });
  t.after(() => act(() => root.unmount()));
  await act(async () => { t.mock.timers.tick(0); });
  return { root, calls, listeners, player: () => player, beforeAudio: () => beforeAudio,
    highlights: () => root.root.findAllByType('Text').filter(node => Array.isArray(node.props.style) &&
      node.props.style.some(style => style?.backgroundColor === colors.accent && style?.color === colors.background)).map(node => node.children[0]),
    tapLine: label => act(() => root.root.findByProps({ accessibilityLabel: label }).props.onPress(event())),
    emit: (event, payload) => act(async () => { for (const listener of [...listeners.get(event) ?? []]) listener(payload); }),
    stop: () => act(async () => root.root.findByProps({ label: 'Stop line audio' }).props.onPress()),
    background: () => act(async () => { for (const listener of apps) listener('background'); }),
  };
}

test('chat highlights successive words using clip-relative playback and clears them on completion or stop', async t => {
  const env = await harness(t);
  assert.equal(env.calls.length, 0, 'opening the reader keeps audio lazy');
  assert.deepEqual(env.highlights(), []);
  await env.tapLine('Play English line at 2:00');
  assert.deepEqual(env.highlights(), ['Hello']);
  assert.equal(env.beforeAudio(), 1);
  assert.equal(env.player().currentTime, 2.5);
  await env.emit('timeUpdate', { currentTime: 2 });
  await env.emit('timeUpdate', { currentTime: NaN });
  assert.deepEqual(env.highlights(), ['Hello'], 'pre-seek and invalid samples cannot move the highlight');
  await env.emit('timeUpdate', { currentTime: 3.5 });
  assert.deepEqual(env.highlights(), ['world']);
  await env.emit('timeUpdate', { currentTime: 4.5 });
  assert.deepEqual(env.highlights(), []);
  assert.equal(env.player().playing, false);
  assert.ok(env.calls.some(command => command.op === 'ai_game_audio_release'));
  await env.tapLine('Play English line at 2:00');
  assert.deepEqual(env.highlights(), ['Hello']);
  await env.stop();
  assert.deepEqual(env.highlights(), []);
});

test('changing lines clears the previous highlight while loading and ignores its late playback samples', async t => {
  let resolve;
  const pending = new Promise(done => { resolve = done; });
  const env = await harness(t, { respond: command => command.cueId === 'cue-1' ? pending : clip('first') });
  await env.tapLine('Play English line at 2:00');
  await env.emit('timeUpdate', { currentTime: 3.75 });
  assert.deepEqual(env.highlights(), ['world']);
  const [previousTick] = env.listeners.get('timeUpdate');
  await env.tapLine('Play English line at 4:00');
  assert.deepEqual(env.highlights(), []);
  await act(() => previousTick({ currentTime: 4 }));
  assert.deepEqual(env.highlights(), []);
  await act(async () => { resolve(clip('second', 0.25)); });
  assert.deepEqual(env.highlights(), ['Try']);
  await env.emit('timeUpdate', { currentTime: 1.25 });
  assert.deepEqual(env.highlights(), ['again']);
  await env.background();
  assert.deepEqual(env.highlights(), []);
  assert.equal(env.player().playing, false);
});

test('switching comparison tracks with the same cue ID uses the new caption and clip clocks', async t => {
  const env = await harness(t, { comparison, respond: command => clip(command.trackId, command.trackId === '.es' ? 0.25 : 2.5) });
  await env.tapLine('Play English line at 2:00');
  await env.emit('timeUpdate', { currentTime: 3.75 });
  assert.deepEqual(env.highlights(), ['world']);
  await env.tapLine('Play Spanish line at 5:00');
  assert.deepEqual(env.highlights(), ['Hola']);
  await env.emit('timeUpdate', { currentTime: 1.25 });
  assert.deepEqual(env.highlights(), ['mundo']);
  const requests = env.calls.filter(command => command.op === 'ai_caption_audio').map(command => command.command);
  assert.deepEqual(requests.map(({ trackId, cueId }) => [trackId, cueId]), [['.en', 'cue-0'], ['.es', 'cue-0']]);
  await env.emit('statusChange', { status: 'error' });
  assert.deepEqual(env.highlights(), []);
});

test('chat captions without word timing still play and stop as a line', async t => {
  const env = await harness(t, { untimed: true });
  await env.tapLine('Play English line at 2:00');
  await env.emit('timeUpdate', { currentTime: 3.75 });
  assert.deepEqual(env.highlights(), []);
  assert.equal(env.player().playing, true);
  await env.emit('playToEnd');
  assert.equal(env.player().playing, false);
});
