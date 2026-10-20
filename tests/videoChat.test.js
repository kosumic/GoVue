import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const first = 'abcdefghijk', second = '0123456789a';
const transcript = videoId => ({ videoId, title: `Video ${videoId}`, trackId: '.en', language: 'English', tracks: [], segments: [{ start: 1, duration: 3, text: 'These captions are the chat context.' }] });

async function harness(videoId, restore = async id => transcript(id)) {
  const calls = [], opened = [], saved = new Map(), audioCalls = [], backHandlers = new Set();
  let closed = 0;
  let chatProps;
  const provider = { id: 'fixture', label: 'Fixture', model: 'first', connected: true, enabled: true, models: [] };
  const ai = { providers: [provider], groups: [], activeId: provider.id, ready: true, busy: '', clear() {}, stop() {}, async select(id, model) { if (model) provider.model = model; return true; } };
  const core = {
    observeCoreUpdates: () => ({ remove() {} }),
    async coreRequest(command) {
      if (command.op === 'transcript_session_load') return { version: command.version, documents: [{ data: command.initial, cached: true }], loading: false, error: null };
      return null;
    },
    async aiRequest(command) {
    if (command.op === 'ai_chat_session_cancel' || command.op === 'ai_chat_session_feedback') return null;
    if (command.op === 'ai_chat_session_forget') return null;
    calls.push(command);
    switch (command.op) {
      case 'ai_chats': return { chats: [...saved.values()] };
      case 'ai_chat_video_open': {
        const id = command.chatId ?? `video_${command.videoId}`;
        const chat = saved.get(id) ?? { id, videoId: command.videoId, title: command.title || 'Video chat', providerId: provider.id, providerLabel: provider.label, model: provider.model, messages: [], transcript: null, activeTask: null };
        if (command.transcript) chat.transcript = transcript(command.videoId);
        saved.set(id, chat); return structuredClone(chat);
      }
      case 'ai_chat_get': return structuredClone(saved.get(command.chatId));
      case 'ai_chat_branch': {
        const source = saved.get(command.chatId);
        const chat = structuredClone({ ...source, id: `branch_${saved.size}`, title: `Branch of ${source.title}`,
          messages: source.messages.slice(0, source.messages.findIndex(message => message.id === command.messageId) + 1) });
        saved.set(chat.id, chat); return structuredClone(chat);
      }
      case 'ai_chat_model': {
        const chat = saved.get(command.chatId); chat.model = provider.model; return structuredClone(chat);
      }
      default: throw Error(`Unexpected operation ${command.op}`);
    }
  } };
  const native = { View: 'View', Text: 'Text', Pressable: 'Pressable', KeyboardAvoidingView: 'KeyboardAvoidingView', BackHandler: { addEventListener(_event, handler) { backHandlers.add(handler); return { remove: () => backHandlers.delete(handler) }; } }, Keyboard: { dismiss() {} }, Alert: { alert() {} }, Platform: { OS: 'android' }, StyleSheet: { create: value => value, hairlineWidth: 1 } };
  const result = await build({ entryPoints: ['src/components/AiSheet.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external', jsx: 'automatic',
    plugins: [{ name: 'chat-edges', setup(builder) {
      builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'mock-core', external: true }));
      builder.onResolve({ filter: /\/core\/transcripts$/ }, () => ({ path: 'mock-transcripts', external: true }));
      builder.onResolve({ filter: /\/ai\/useAi$/ }, () => ({ path: 'mock-ai', external: true }));
      builder.onResolve({ filter: /\/ai\/useGameAudio$/ }, () => ({ path: 'mock-audio', external: true }));
      builder.onResolve({ filter: /\/(AiChat|AiProviders|AiModels|AiUsageSheet|AiReasoning|AiHistoryDrawer|GamePractice|IconButton|TranscriptPanel)$/ }, args => ({ path: `mock-component/${args.path.split('/').at(-1)}`, external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => {
    if (name === 'mock-core') return core;
    if (name === 'react-native') return native;
    if (name === 'react-native-safe-area-context') return { SafeAreaView: 'SafeAreaView' };
    if (name === 'lucide-react-native') return Object.fromEntries(['ChevronDown', 'ChevronLeft', 'Menu', 'Play', 'SquarePen', 'X'].map(key => [key, key]));
    if (name === 'mock-ai') return { useAi: () => ai };
    if (name === 'mock-audio') return { useTranscriptAudio: scope => ({ phase: 'idle', error: '', stop() {}, async play(cueId) { audioCalls.push({ ...scope, cueId }); } }) };
    if (name === 'mock-transcripts') return { restoreTranscript: restore, extractTranscript: async () => { throw Error('Unexpected extraction'); } };
    if (name.startsWith('mock-component/')) {
      const component = name.split('/')[1];
      if (component === 'AiReasoning') return { reasoningName: value => value || 'Default' };
      return { [component]: component === 'AiChat' ? props => { chatProps = props; return createElement('AiChat'); } : component };
    }
    return require(name);
  }, module, module.exports);
  const props = { visible: true, videoId, transcript: null, onClose() { closed++; }, onOpenVideo: id => opened.push(id) };
  let root;
  await act(async () => { root = create(createElement(module.exports.AiSheet, props)); });
  return { calls, saved, opened, audioCalls, props: () => chatProps,
    closed: () => closed, backHandlers,
    back: () => act(() => { for (const handler of [...backHandlers].reverse()) if (handler()) break; }),
    render: changes => act(async () => { root.update(createElement(module.exports.AiSheet, { ...props, ...changes })); }),
    unmount: () => act(() => root.unmount()), root };
}

test('chat shares the player window and Android Back closes chat panels before leaving chat', async t => {
  const env = await harness(first);
  t.after(env.unmount);
  assert.equal(env.root.root.findAllByType('Modal').length, 0, 'chat must share the window with the floating VideoView');
  await act(() => env.props().onOpenTranscript());
  await env.back();
  assert.equal(env.root.root.findAllByType('TranscriptPanel').length, 0);
  assert.equal(env.closed(), 0);
  await act(() => env.props().onModels());
  assert.equal(env.root.root.findAllByType('AiModels').length, 1);
  await env.back();
  assert.equal(env.root.root.findAllByType('AiModels').length, 0);
  assert.equal(env.closed(), 0);
  await act(() => env.root.root.findByProps({ label: 'Open conversations' }).props.onPress());
  assert.equal(env.root.root.findByType('AiHistoryDrawer').props.visible, true);
  await env.back();
  assert.equal(env.root.root.findByType('AiHistoryDrawer').props.visible, false);
  assert.equal(env.closed(), 0);
  await env.back();
  assert.equal(env.closed(), 1);
  await env.render({ visible: false });
  assert.equal(env.root.toJSON(), null);
  assert.equal(env.backHandlers.size, 0, 'hidden chat must release Back to the browser');
});

test('opening a video chat attaches context and changing its model preserves the empty conversation without inference', async t => {
  const env = await harness(first);
  t.after(env.unmount);
  assert.equal(env.props().chat.conversation.videoId, first);
  assert.equal(env.props().contextReady, true);
  assert.equal(env.props().attachment.videoId, first);
  assert.equal(env.props().onDetach, undefined);
  assert.deepEqual(env.props().chat.conversation.messages, []);
  assert.equal(env.calls.filter(c => c.op === 'ai_chat_send').length, 0);
  assert.equal(env.saved.size, 1);
  await act(() => env.root.root.findByProps({ accessibilityLabel: 'Open video' }).props.onPress());
  assert.deepEqual(env.opened, [first]);
  const beforeReader = env.calls.length;
  await act(() => env.props().onOpenTranscript());
  const reader = env.root.root.findByType('TranscriptPanel');
  assert.deepEqual(reader.props.data, transcript(first));
  assert.equal(reader.props.onSeek, undefined);
  assert.equal(reader.props.onExtract, undefined);
  assert.equal(env.audioCalls.length, 0, 'opening the transcript does not prepare audio');
  await act(() => reader.props.onPlayLine(0));
  assert.deepEqual(env.audioCalls, [{ chatId: `video_${first}`, videoId: first, trackId: '.en', cueId: 'cue-0' }]);
  await act(() => reader.props.onClose());
  assert.equal(env.root.root.findAllByType('TranscriptPanel').length, 0);
  assert.equal(env.calls.length, beforeReader, 'reading the attached captions does not reload or interrupt the chat');
  await act(() => env.props().onModels());
  const models = env.root.root.findByType('AiModels');
  await act(async () => { models.props.onSelect('fixture', 'second'); });
  assert.equal(env.props().chat.conversation.id, `video_${first}`);
  assert.equal(env.props().chat.conversation.model, 'second');
  assert.equal(env.props().attachment.videoId, first);
  assert.equal(env.saved.size, 1);
  assert.equal(env.calls.filter(c => c.op === 'ai_chat_send').length, 0);
});

test('a late transcript cannot attach to a replacement video chat', async t => {
  let resolveFirst;
  const pending = new Promise(resolve => { resolveFirst = resolve; });
  const env = await harness(first, id => id === first ? pending : Promise.resolve(transcript(id)));
  t.after(env.unmount);
  assert.equal(env.props().contextReady, false);
  await env.render({ videoId: second });
  assert.equal(env.props().attachment.videoId, second);
  await act(async () => { resolveFirst(transcript(first)); });
  assert.equal(env.props().chat.conversation.videoId, second);
  assert.equal(env.props().attachment.videoId, second);
  assert.equal(env.props().contextReady, true);
  await act(() => env.root.root.findByProps({ accessibilityLabel: 'Open video' }).props.onPress());
  assert.deepEqual(env.opened, [second]);
  assert.equal(env.calls.filter(c => c.op === 'ai_chat_video_open' && c.transcript?.videoId === first).length, 0);
  assert.equal(env.calls.filter(c => c.op === 'ai_chat_send').length, 0);
});

test('video branches keep their own history, transcript, and identity when reopened', async t => {
  const env = await harness(first); t.after(env.unmount);
  const sourceId = `video_${first}`;
  const source = env.saved.get(sourceId);
  source.messages = ['First question', 'First answer', 'Later question', 'Later answer'].map((content, index) => ({
    id: `message_${index}`, role: index % 2 ? 'assistant' : 'user', status: 'completed', content,
  }));
  await act(async () => env.props().chat.open(sourceId));
  await act(async () => env.props().onBranch('message_1'));
  const branchId = env.props().chat.conversation.id;
  assert.notEqual(branchId, sourceId);
  assert.equal(env.props().contextReady, true);
  assert.equal(env.props().attachment.videoId, first);
  assert.equal(env.props().chat.conversation.messages.length, 2);
  assert.equal(env.saved.get(sourceId).messages.length, 4);
  const drawer = () => env.root.root.findByType('AiHistoryDrawer');
  assert.ok(drawer().props.chats.some(chat => chat.id === branchId));
  await act(async () => drawer().props.onOpen(sourceId));
  assert.equal(env.props().chat.conversation.id, sourceId);
  assert.equal(env.props().chat.conversation.messages.length, 4);
  await act(async () => drawer().props.onOpen(branchId));
  assert.equal(env.props().chat.conversation.id, branchId);
  assert.equal(env.props().chat.conversation.messages.length, 2);
  await env.render({ visible: false });
  await env.render({ visible: true });
  assert.equal(env.props().chat.conversation.id, branchId);
  assert.equal(env.props().contextReady, true);
  assert.equal(env.calls.filter(command => command.op === 'ai_chat_send').length, 0);
});
