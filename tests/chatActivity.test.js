import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement as h, forwardRef, useImperativeHandle } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
globalThis.requestAnimationFrame = callback => setImmediate(callback);
globalThis.cancelAnimationFrame = clearImmediate;
const require = createRequire(import.meta.url);
const noop = () => {};
const colors = new Proxy({}, { get: () => '#777777' });
const native = {
  View: 'View', Text: 'Text', TextInput: 'TextInput', Pressable: 'Pressable', ScrollView: 'ScrollView', ActivityIndicator: 'ActivityIndicator',
  StyleSheet: { create: value => value, hairlineWidth: 1 },
  useWindowDimensions: () => ({ width: 400, height: 800 }),
  AccessibilityInfo: { isReduceMotionEnabled: async () => true, addEventListener: () => ({ remove: noop }) },
  Animated: { View: 'AnimatedView', Value: class { interpolate() { return 1; } }, timing: () => ({ start: noop, stop: noop }) },
  FlatList: forwardRef(function FlatList({ data, renderItem, ListEmptyComponent }, ref) {
    useImperativeHandle(ref, () => ({ scrollToEnd: noop }));
    return h('FlatList', null, data.length ? data.map((item, index) => h(React.Fragment, { key: item.id }, renderItem({ item, index }))) : ListEmptyComponent);
  }),
};
const mocks = {
  'react-native': native,
  'lucide-react-native': new Proxy({}, { get: (_, name) => props => h(`Icon${String(name)}`, props) }),
  '@react-native-clipboard/clipboard': { setString: noop },
  theme: { useThemeColors: () => colors, useThemeStyles: factory => factory(colors) },
  AiMarkdown: { AiMarkdown: props => h('Markdown', props) },
  AiReasoning: { AiReasoning: () => null },
  LearningGameCard: { LearningGameCard: props => h('GameCard', { gameId: props.game.id, enabled: props.enabled }) },
  useVoiceInput: { useVoiceInput: () => ({ active: false, disabled: true, phase: 'idle', interimText: '', changeDraft: noop, toggle: noop }) },
  PlayerSheet: { PlayerSheet: ({ visible, children, title }) => visible ? h('Sheet', { title }, children) : null },
};
const bundled = await build({ entryPoints: ['src/components/AiChat.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
  plugins: [{ name: 'activity-ui', setup(builder) {
    builder.onResolve({ filter: /^(react-native|lucide-react-native|@react-native-clipboard\/clipboard)$|\/(theme|AiMarkdown|AiReasoning|LearningGameCard|useVoiceInput|PlayerSheet)$/ }, args => {
      const key = mocks[args.path] ? args.path : args.path.split('/').at(-1);
      return { path: `mock:${key}`, external: true };
    });
  } }],
});
const module = { exports: {} };
Function('require', 'module', 'exports', bundled.outputFiles[0].text)(name => name.startsWith('mock:') ? mocks[name.slice(5)] : require(name), module, module.exports);
const { AiChat } = module.exports;
const game = { id: 'game', definition: { html: 'PRIVATE_GAME_SOURCE', rules: 'PRIVATE_ANSWERS' } };
const tool = { id: 'tool', kind: 'tool', callId: 'call', name: 'create_interactive_game', startedAt: 1000, durationMs: 2500, status: 'completed', gameId: 'game', title: 'Greeting exercise' };
const text = (id, content, status = 'completed') => ({ id, kind: 'text', content, status });
const reasoning = status => ({ id: 'reason', kind: 'reasoning', content: 'Provider reasoning', status });
const reply = blocks => ({ id: 'reply', role: 'assistant', content: 'BeforeAfter', status: 'completed', blocks, games: [game] });
const props = (message, busy = false) => ({
  chat: { conversation: { id: 'chat', messages: [message] }, busy, settingsBusy: false, error: '', gameAction: noop, stop: noop },
  draft: '', onDraftChange: noop, attachment: null, availableTranscript: null, connected: true, loading: false, voiceEnabled: false,
  onSend: noop, onRetry: noop, onAttach: noop, onOpenTranscript: noop, onSummarize: noop, onModels: noop, reasoning: 'Low',
});
async function render(t, message, busy = false) {
  let root;
  await act(async () => { root = create(h(AiChat, props(message, busy))); });
  t.after(() => act(async () => root.unmount()));
  return { root, update: (message, busy) => act(async () => root.update(h(AiChat, props(message, busy)))) };
}
const labels = root => root.root.findAllByType('Text').map(node => node.children.join(''));

test('ordered activity renders each answer segment and its exercise at the tool position, with details behind a tap', async t => {
  const { root } = await render(t, reply([reasoning('completed'), text('before', 'Before'), tool, text('after', 'After')]));
  assert.deepEqual(root.root.findAll(node => node.type === 'Markdown' || node.type === 'GameCard').map(node => node.type === 'Markdown' ? node.props.content : node.props.gameId), ['Before', 'game', 'After']);
  assert.equal(root.root.findAllByType('Sheet').length, 0);
  assert.equal(root.root.findAllByType('Markdown').some(node => node.props.content === 'Provider reasoning'), false);
  const button = root.root.findAllByType('Pressable').find(node => node.props.accessibilityLabel?.includes('Exercise ready'));
  await act(async () => button.props.onPress());
  assert.equal(root.root.findByType('Sheet').props.title, 'Tool activity');
  assert.ok(labels(root).includes('create_interactive_game'));
  assert.ok(labels(root).includes('Greeting exercise'));
  assert.ok(labels(root).includes('Completed'));
  assert.ok(labels(root).includes('2.5s'));
  assert.equal(JSON.stringify(root.toJSON()).includes('PRIVATE_ANSWERS'), false);
  assert.equal(JSON.stringify(root.toJSON()).includes('PRIVATE_GAME_SOURCE'), false);
});

test('reasoning opens on request, folds when its phase ends, and a stopped tool loses its spinner immediately', async t => {
  const { root, update } = await render(t, { ...reply([reasoning('running')]), games: [], status: 'streaming' }, true);
  const thinking = root.root.findByProps({ accessibilityLabel: 'Thinking' });
  assert.equal(thinking.props.accessibilityState.expanded, false);
  await act(async () => thinking.props.onPress());
  assert.equal(root.root.findByType('Markdown').props.content, 'Provider reasoning');
  const pending = { ...tool, status: 'preparing', durationMs: 0, gameId: undefined };
  await update({ ...reply([reasoning('completed'), pending]), games: [], status: 'streaming' }, true);
  assert.equal(root.root.findByProps({ accessibilityLabel: 'Reasoning' }).props.accessibilityState.expanded, false);
  assert.equal(root.root.findAllByType('Markdown').length, 0);
  assert.ok(labels(root).includes('Creating exercise…'));
  assert.equal(root.root.findAllByType('ActivityIndicator').length, 1, 'the tool supplies the only progress indicator');
  await update({ ...reply([reasoning('completed'), pending]), games: [], status: 'stopped' }, false);
  assert.ok(labels(root).includes('Exercise creation stopped'));
  assert.equal(root.root.findAllByType('ActivityIndicator').length, 0);
});

test('legacy messages retain their answer, optional reasoning, and exercise', async t => {
  const { root } = await render(t, { ...reply(undefined), reasoning: 'Legacy reasoning', content: 'Legacy answer' });
  assert.equal(root.root.findByType('Markdown').props.content, 'Legacy answer');
  assert.equal(root.root.findByType('GameCard').props.gameId, 'game');
  await act(async () => root.root.findByProps({ accessibilityLabel: 'Reasoning' }).props.onPress());
  assert.deepEqual(root.root.findAllByType('Markdown').map(node => node.props.content), ['Legacy reasoning', 'Legacy answer']);
});
