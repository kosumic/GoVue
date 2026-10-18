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
const copiedTexts = [], hapticEvents = [];
const colors = new Proxy({}, { get: () => '#777777' });
const native = {
  Platform: { OS: 'android' },
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
  'react-native-enriched-markdown': { EnrichedMarkdownText: props => h('SelectableText', props) },
  'lucide-react-native': new Proxy({}, { get: (_, name) => props => h(`Icon${String(name)}`, props) }),
  '@react-native-clipboard/clipboard': { setString: value => copiedTexts.push(value) },
  theme: { useThemeColors: () => colors, useThemeStyles: factory => factory(colors) },
  AiMarkdown: { AiMarkdown: props => h('Markdown', props) },
  AiReasoning: { AiReasoning: () => null },
  LearningGameCard: { LearningGameCard: props => h('GameCard', { gameId: props.game.id, active: props.active, enabled: props.enabled, onActivate: props.onActivate, onDeactivate: props.onDeactivate }) },
  useVoiceInput: { useVoiceInput: ({ onDraftChange }) => ({ active: false, disabled: true, phase: 'idle', interimText: '', changeDraft: onDraftChange, toggle: noop }) },
  useExampleSpeech: { useResponseSpeech: () => ({ active: null, error: '', speak: noop, stop: async () => {} }) },
  useGameAudio: { stopCaptionAudio: noop },
  PlayerSheet: { PlayerSheet: ({ visible, children, title }) => visible ? h('Sheet', { title }, children) : null },
  Haptics: { useHaptic: () => () => hapticEvents.push('copy') },
  'lingoplay-core': { aiRequest: noop },
};
const bundled = await build({ entryPoints: ['src/components/AiChat.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
  plugins: [{ name: 'activity-ui', setup(builder) {
    builder.onResolve({ filter: /^(react-native|react-native-enriched-markdown|lucide-react-native|@react-native-clipboard\/clipboard)$|\/(theme|AiMarkdown|AiReasoning|LearningGameCard|useVoiceInput|useExampleSpeech|useGameAudio|PlayerSheet|Haptics|lingoplay-core)$/ }, args => {
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

test('the latest game opens automatically, stays closed on text updates, and yields to a newly created game', async t => {
  let root;
  const old = { ...reply(undefined), id: 'old', games: [{ ...game, id: 'old-game' }] };
  const fresh = { ...reply([tool]), id: 'fresh', status: 'streaming' };
  const show = (messages, chatId = 'chat') => act(async () => {
    const value = props(messages.at(-1), true);
    value.voiceEnabled = true;
    value.chat.conversation = { id: chatId, messages };
    if (root) root.update(h(AiChat, value));
    else root = create(h(AiChat, value));
  });
  t.after(() => act(async () => root?.unmount()));
  const cards = () => root.root.findAllByType('GameCard');
  const active = () => cards().filter(card => card.props.active).map(card => card.props.gameId);
  await show([old]);
  assert.deepEqual(active(), ['old-game'], 'opening a saved chat opens its latest game');
  await show([old, fresh]);
  assert.deepEqual(active(), ['game']);
  assert.equal(cards().at(-1).props.enabled, true, 'a committed game is playable while the answer continues streaming');
  await act(async () => cards().at(-1).props.onDeactivate());
  await show([old, { ...fresh, content: 'The rest of the answer', status: 'completed' }]);
  assert.deepEqual(active(), [], 'a later response snapshot must respect Close');
  await act(async () => cards()[0].props.onActivate());
  assert.deepEqual(active(), ['old-game'], 'older games remain available on request');
  await show([old, fresh, { ...reply(undefined), id: 'newest', games: [{ ...game, id: 'new-game' }] }]);
  assert.deepEqual(active(), ['new-game'], 'only one game surface is selected');
  await show([old], 'another-chat');
  assert.deepEqual(active(), ['old-game'], 'changing conversations selects that chat’s latest game');
});

test('copy confirms success with a haptic and resets its checkmark timer on repeat', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  copiedTexts.length = 0; hapticEvents.length = 0;
  const { root } = await render(t, { ...reply(undefined), games: [] });
  await act(async () => root.root.findByProps({ accessibilityLabel: 'Copy response' }).props.onPress());
  assert.deepEqual(copiedTexts, ['BeforeAfter']);
  assert.deepEqual(hapticEvents, ['copy']);
  await act(async () => t.mock.timers.tick(1000));
  await act(async () => root.root.findByProps({ accessibilityLabel: 'Response copied' }).props.onPress());
  await act(async () => t.mock.timers.tick(1000));
  assert.ok(root.root.findByProps({ accessibilityLabel: 'Response copied' }));
  await act(async () => t.mock.timers.tick(500));
  assert.ok(root.root.findByProps({ accessibilityLabel: 'Copy response' }));
  assert.equal(hapticEvents.length, 2);
});

test('every response footer regenerates its own reply and disables generation when unavailable', async t => {
  const messages = [
    { id: 'first-prompt', role: 'user', content: 'First question', status: 'completed' },
    { ...reply(undefined), id: 'first-reply', games: [] },
    { id: 'second-prompt', role: 'user', content: 'Second question', status: 'completed' },
    { ...reply(undefined), id: 'second-reply', games: [] },
    { id: 'third-prompt', role: 'user', content: 'Third question', status: 'completed' },
    { ...reply(undefined), id: 'third-reply', status: 'failed', games: [] },
  ];
  const retried = [];
  const value = { ...props(messages.at(-1)), onRetry: id => retried.push(id) };
  value.chat.conversation.messages = messages;
  let root;
  await act(async () => { root = create(h(AiChat, value)); });
  t.after(() => act(async () => root.unmount()));
  const buttons = () => root.root.findAllByType('Pressable').filter(node => ['Regenerate response', 'Try again'].includes(node.props.accessibilityLabel));
  assert.equal(buttons().length, 3, 'each assistant response has a button, including older and failed replies');
  assert.equal(buttons()[0].props.accessibilityHint, 'Replaces this response and later messages');
  for (const button of buttons()) await act(async () => button.props.onPress());
  assert.deepEqual(retried, ['first-reply', 'second-reply', 'third-reply']);
  for (const unavailable of [
    { chat: { ...value.chat, busy: true } }, { loading: true }, { connected: false },
    { contextReady: false }, { chat: { ...value.chat, settingsBusy: true } },
  ]) {
    await act(async () => root.update(h(AiChat, { ...value, ...unavailable })));
    assert.ok(buttons().every(button => button.props.disabled && button.props.accessibilityState.disabled));
  }
  await act(async () => root.update(h(AiChat, { ...value, chat: { ...value.chat, conversation: { ...value.chat.conversation, messages: messages.map(message => message.id === 'third-reply' ? { ...message, status: 'streaming' } : message) } } })));
  assert.equal(buttons().at(-1).props.disabled, true, 'a streaming reply cannot regenerate even if the busy flag is stale');
  assert.equal(buttons()[0].props.disabled, false);
});

test('editing loads the original prompt, cancel restores the draft, and failed sends keep the edit', async t => {
  const message = { id: 'original', role: 'user', content: 'Original prompt', status: 'completed' };
  const sends = [];
  let accepted = false, root;
  function Editor() {
    const [draft, onDraftChange] = React.useState('Unsent draft');
    return h(AiChat, { ...props(message), draft, onDraftChange, onSend: async id => { sends.push({ id, draft }); return accepted; } });
  }
  await act(async () => { root = create(h(Editor)); });
  t.after(() => act(async () => root.unmount()));
  const press = label => act(async () => root.root.findByProps({ accessibilityLabel: label }).props.onPress());
  const input = () => root.root.findByType('TextInput');
  await press('Edit message');
  assert.equal(input().props.value, 'Original prompt');
  assert.ok(labels(root).includes('Sending replaces later messages and replies.'));
  await press('Cancel edit');
  assert.equal(input().props.value, 'Unsent draft');
  await press('Edit message');
  await act(async () => input().props.onChangeText('Revised prompt'));
  await press('Send edited message');
  assert.deepEqual(sends, [{ id: 'original', draft: 'Revised prompt' }]);
  assert.ok(root.root.findByProps({ accessibilityLabel: 'Cancel edit' }));
  accepted = true;
  await press('Send edited message');
  assert.equal(root.root.findAllByProps({ accessibilityLabel: 'Cancel edit' }).length, 0);
});

test('response details show reported counts and preserve unknown usage', async t => {
  const { root, update } = await render(t, { ...reply(undefined), games: [], model: 'saved-model', providerLabel: 'saved-provider', usage: { inputTokens: 100, outputTokens: 20, reasoningTokens: 5, cacheReadTokens: 40 }, usageDetails: { totalTokens: 120, cacheStatus: 'hit', cacheHitRate: 40, outputTokensPerSecond: 10 } });
  assert.ok(labels(root).some(label => label.includes('120 tokens')));
  await act(async () => root.root.findByProps({ accessibilityLabel: 'Response details' }).props.onPress());
  for (const label of ['saved-model', 'saved-provider', 'Input (including cache)', 'Reasoning (included in output)', '120', 'Cache hit · 40%', '10.0 tok/s']) assert.ok(labels(root).includes(label), label);
  await update({ ...reply(undefined), games: [] }, false);
  assert.ok(labels(root).some(label => label.includes('Usage not reported')));
  assert.ok(labels(root).includes('Cache unknown'));
  assert.equal(labels(root).includes('Total tokens'), false, 'unreported usage is not zero');
});
