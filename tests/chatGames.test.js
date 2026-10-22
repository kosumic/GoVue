import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const game = (revision = 0) => ({ id: 'game', definition: { title: 'An untrusted title' }, state: { count: revision }, revision });
const saved = (revision = 0, streaming = false) => ({
  id: 'chat', title: 'Fixture', providerId: 'fixture', model: 'fixture', transcript: null,
  activeTask: streaming ? 'task' : null,
  messages: [
    { id: 'user', role: 'user', content: 'Make a game', status: 'completed' },
    { id: 'game-message', role: 'assistant', content: 'Play', status: 'completed', games: [game(revision)] },
    ...(streaming ? [{ id: 'next-user', role: 'user', content: 'Explain', status: 'completed' }, { id: 'reply', role: 'assistant', content: '', status: 'streaming' }] : []),
  ],
});

async function harness(t) {
  const calls = [];
  let feedbackReady = false;
  let controller;
  const result = await build({ entryPoints: ['src/ai/useChats.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'game-core', setup(builder) { builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'test-core', external: true })); } }],
  });
  const core = { observeCoreUpdates() { return { remove() {} }; }, aiRequest(command) {
    if (command.op === 'ai_chat_session_cancel') return Promise.resolve(null);
    if (command.op === 'ai_chat_session_forget') return Promise.resolve({});
    if (command.op === 'ai_chat_session_feedback' && !feedbackReady) return Promise.resolve(null);
    if (command.op === 'ai_chat_session_send') command = { ...command.command, op: 'ai_chat_send' };
    if (command.op === 'ai_chats') return Promise.resolve({ chats: [] });
    const entry = { command, answered: false };
    calls.push(entry);
    if (['ai_cancel', 'ai_forget'].includes(command.op)) return Promise.resolve({});
    return new Promise(resolve => { entry.resolve = value => { entry.answered = true; resolve(value); }; });
  } };
  const module = { exports: {} };
  const app = { currentState: 'active', addEventListener() { return { remove() {} }; } };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => name === 'test-core' ? core : name === 'react-native' ? { AppState: app } : require(name), module, module.exports);
  function Chat() { controller = module.exports.useChats(true, ''); return null; }
  let root;
  await act(async () => { root = create(createElement(Chat)); });
  t.after(() => act(async () => root.unmount()));
  const pending = op => calls.find(entry => entry.command.op === op && !entry.answered);
  const reply = (op, value) => act(async () => { const entry = pending(op); assert.ok(entry, op); if (op === 'ai_game_action' && value.notify) feedbackReady = true; entry.resolve(value); });
  let opening;
  await act(() => { opening = controller.open('chat'); });
  await reply('ai_chat_get', saved());
  await opening;
  return { calls, chat: () => controller, pending, reply };
}

test('game actions serialize revisions and ask Rust for feedback after inference', async t => {
  const env = await harness(t);
  let sending;
  await act(() => { sending = env.chat().send('fixture', 'Explain', null); });
  await env.reply('ai_chat_send', { taskId: 'task', conversation: saved(0, true) });
  await sending;
  let first, second;
  await act(() => {
    first = env.chat().gameAction('game-message', 'game', 'choose', { id: 'a' }, 'tap-1');
    second = env.chat().gameAction('game-message', 'game', 'check', {}, 'tap-2');
  });
  assert.equal(env.pending('ai_game_action').command.revision, 0);
  assert.equal(env.calls.filter(c => c.command.op === 'ai_game_action').length, 1);
  await env.reply('ai_game_action', { game: game(1), notify: false });
  await first;
  assert.equal(env.pending('ai_game_action').command.revision, 1);
  await env.reply('ai_game_action', { game: game(2), notify: true });
  await second;
  assert.equal(env.calls.filter(c => c.command.op === 'ai_chat_send').length, 1, 'a submit must not interrupt the current response');
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 1, state: { status: 'completed', text: 'Explained.' } });
  const older = saved(2, true);
  older.activeTask = null;
  older.messages.at(-1).status = 'completed';
  await env.reply('ai_chat_get', older);
  assert.equal(env.chat().conversation.messages[1].games[0].revision, 2, 'late inference snapshots cannot erase game progress');
  const feedback = env.pending('ai_chat_session_feedback');
  assert.ok(feedback, 'submission starts a new agent turn after inference finishes');
  assert.equal(feedback.command.chatId, 'chat');
  assert.equal(feedback.command.prompt, undefined, 'Rust owns the feedback prompt');
  assert.equal(env.calls.some(c => c.command.op === 'ai_cancel'), false);
  await env.reply('ai_chat_session_feedback', { taskId: 'feedback', conversation: saved(2, true) });
});

test('a late game action cannot reopen a conversation after the user leaves it', async t => {
  const env = await harness(t);
  let action;
  await act(() => { action = env.chat().gameAction('game-message', 'game', 'check', {}, 'submit'); });
  const failure = assert.rejects(action, /no longer open/);
  await act(() => env.chat().newChat());
  await env.reply('ai_game_action', { game: game(1), notify: true });
  await failure;
  assert.equal(env.chat().conversation, null);
  assert.equal(env.pending('ai_chat_send'), undefined);
});
