import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const conversation = (reasoningEffort = 'low', content = '', status = 'streaming', replyEffort = 'low') => ({
  id: 'chat', title: 'Fixture', providerId: 'fixture', model: 'fixture', reasoningEffort,
  activeTask: status === 'streaming' ? 'task' : null,
  messages: [{ id: 'user', role: 'user', content: 'Hello', status: 'completed' }, { id: 'reply', role: 'assistant', content, status, reasoningEffort: replyEffort }],
});

async function harness() {
  const calls = [];
  let controller;
  const result = await build({ entryPoints: ['src/ai/useChats.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'chat-core', setup(builder) {
      builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'test-core', external: true }));
    } }],
  });
  const core = { aiRequest(command) {
    if (command.op === 'ai_chats') return Promise.resolve({ chats: [] });
    const entry = { command, answered: false };
    calls.push(entry);
    if (['ai_cancel', 'ai_forget'].includes(command.op)) return Promise.resolve({});
    return new Promise(resolve => { entry.resolve = value => { entry.answered = true; resolve(value); }; });
  } };
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => name === 'test-core' ? core : require(name), module, module.exports);
  function Chat() { controller = module.exports.useChats(true, ''); return null; }
  let root;
  await act(async () => { root = create(createElement(Chat)); });
  return { calls, chat: () => controller, pending: op => calls.find(entry => entry.command.op === op && !entry.answered),
    reply: (op, value) => act(async () => { const entry = calls.find(entry => entry.command.op === op && !entry.answered); assert.ok(entry, op); entry.resolve(value); }),
    unmount: () => act(async () => root.unmount()) };
}

const nextPoll = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 180)); });

test('branch creation switches to the saved copy once and ignores completion after navigation', async t => {
  const env = await harness(); t.after(env.unmount);
  const original = conversation('high', 'Answer', 'completed');
  await act(() => { void env.chat().open('chat'); });
  await env.reply('ai_chat_get', original);
  let pending, duplicate;
  await act(() => { pending = env.chat().branch('reply'); duplicate = env.chat().branch('reply'); });
  assert.equal(await duplicate, undefined);
  assert.equal(env.chat().loading, true);
  assert.deepEqual(env.pending('ai_chat_branch').command, { op: 'ai_chat_branch', chatId: 'chat', messageId: 'reply' });
  const branched = { ...original, id: 'branch', title: 'Branch of Fixture' };
  await env.reply('ai_chat_branch', branched);
  assert.equal((await pending).id, 'branch');
  assert.equal(env.chat().conversation.id, 'branch');
  assert.equal(env.chat().loading, false);
  assert.equal(env.calls.some(entry => entry.command.op === 'ai_chat_send'), false);
  await act(() => { pending = env.chat().branch('reply'); });
  await act(() => env.chat().newChat());
  await env.reply('ai_chat_branch', { ...branched, id: 'late-branch' });
  assert.equal(await pending, undefined);
  assert.equal(env.chat().conversation, null);
  assert.equal(env.chat().loading, false);
});

test('messages sent during startup queue in order and streamed steering switches to the new reply', async t => {
  const env = await harness(); t.after(env.unmount);
  let first, followup, second;
  await act(() => { first = env.chat().send('fixture', 'Hello', null); });
  await act(() => { followup = env.chat().send('fixture', 'Use Spanish.', null); second = env.chat().send('fixture', 'Keep it short.', null); });
  assert.equal(env.calls.filter(entry => entry.command.op === 'ai_chat_send').length, 1, 'startup must finish before steering is submitted');
  await env.reply('ai_chat_send', { taskId: 'task', conversation: conversation() });
  assert.equal(await first, true);
  assert.equal(env.pending('ai_chat_send').command.prompt, 'Use Spanish.');
  const pending = [{ id: 'steer', content: 'Use Spanish.' }];
  await env.reply('ai_chat_send', { taskId: 'task', conversation: { ...conversation(), pendingMessages: pending } });
  assert.equal(await followup, true);
  assert.equal(env.pending('ai_chat_send').command.prompt, 'Keep it short.');
  pending.push({ id: 'brief', content: 'Keep it short.' });
  await env.reply('ai_chat_send', { taskId: 'task', conversation: { ...conversation(), pendingMessages: [...pending] } });
  assert.equal(await second, true);
  assert.equal(env.calls.some(entry => entry.command.op === 'ai_cancel'), false);
  assert.equal(env.calls.filter(entry => entry.command.op === 'ai_chat_poll').length, 1, 'queueing keeps one poller');
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 1, state: { status: 'running', replyId: 'reply', text: 'Original progress', pendingMessages: pending } });
  assert.equal(env.chat().conversation.messages[1].content, 'Original progress');
  await nextPoll();
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 2, state: { status: 'running', replyId: 'steered-reply', text: 'Hola', pendingMessages: [] } });
  const steered = conversation('low', 'Original progress', 'completed');
  steered.activeTask = 'task';
  steered.messages.push({ id: 'steer', role: 'user', content: 'Use Spanish.\n\nKeep it short.', status: 'completed' }, { id: 'steered-reply', role: 'assistant', content: '', status: 'streaming' });
  await env.reply('ai_chat_get', steered);
  assert.equal(env.chat().conversation.messages[1].content, 'Original progress');
  assert.equal(env.chat().conversation.messages[3].content, 'Hola');
  assert.deepEqual(env.chat().conversation.pendingMessages, []);
  assert.equal(env.chat().busy, true);
  await nextPoll();
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 3, state: { status: 'completed', replyId: 'steered-reply', text: 'Hola mundo', pendingMessages: [] } });
  const completed = { ...steered, activeTask: null, messages: steered.messages.map(m => m.id === 'steered-reply' ? { ...m, content: 'Hola mundo', status: 'completed' } : m) };
  await env.reply('ai_chat_get', completed);
  assert.equal(env.chat().busy, false);
  assert.equal(env.chat().conversation.messages[3].content, 'Hola mundo');
});

test('a queue acknowledgment arriving after completion cannot resurrect the old task', async t => {
  const env = await harness(); t.after(env.unmount);
  let first, followup;
  await act(() => { first = env.chat().send('fixture', 'Hello', null); });
  await env.reply('ai_chat_send', { taskId: 'task', conversation: conversation() }); await first;
  await act(() => { followup = env.chat().send('fixture', 'Use Spanish.', null); });
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 1, state: { status: 'completed', replyId: 'steered-reply', text: 'Hola', pendingMessages: [] } });
  const completed = conversation('low', 'Original progress', 'completed');
  completed.messages.push({ id: 'steer', role: 'user', content: 'Use Spanish.', status: 'completed' }, { id: 'steered-reply', role: 'assistant', content: 'Hola', status: 'completed' });
  await env.reply('ai_chat_get', completed);
  const polls = env.calls.filter(entry => entry.command.op === 'ai_chat_poll').length;
  await env.reply('ai_chat_send', { taskId: 'task', conversation: { ...conversation(), pendingMessages: [{ id: 'steer', content: 'Use Spanish.' }] } });
  assert.equal(await followup, true);
  assert.equal(env.chat().busy, false);
  assert.deepEqual(env.chat().conversation, completed);
  assert.equal(env.calls.filter(entry => entry.command.op === 'ai_chat_poll').length, polls);
});

test('a follow-up racing with turn completion starts one new task and ignores late old output', async t => {
  const env = await harness(); t.after(env.unmount);
  let first, followup;
  await act(() => { first = env.chat().send('fixture', 'Hello', null); });
  await env.reply('ai_chat_send', { taskId: 'task', conversation: conversation() }); await first;
  await act(() => { followup = env.chat().send('fixture', 'Use Spanish.', null); });
  const next = conversation('low', 'Original answer', 'completed');
  next.activeTask = 'next-task';
  next.messages.push({ id: 'steer', role: 'user', content: 'Use Spanish.', status: 'completed' }, { id: 'steered-reply', role: 'assistant', content: '', status: 'streaming' });
  await env.reply('ai_chat_send', { taskId: 'next-task', conversation: next });
  assert.equal(await followup, true);
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 1, state: { status: 'completed', replyId: 'reply', text: 'Late old answer' } });
  assert.equal(env.chat().conversation.messages[1].content, 'Original answer');
  assert.equal(env.chat().busy, true);
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 1, state: { status: 'completed', replyId: 'steered-reply', text: 'Hola' } });
  await env.reply('ai_chat_get', { ...next, activeTask: null, messages: next.messages.map(m => m.id === 'steered-reply' ? { ...m, content: 'Hola', status: 'completed' } : m) });
  assert.equal(env.chat().busy, false);
  assert.equal(env.chat().conversation.messages[3].content, 'Hola');
});

test('reasoning changes during request startup apply to the next turn and preserve newer streaming and completion data', async t => {
  const env = await harness();
  t.after(env.unmount);
  await act(async () => { assert.equal(await env.chat().setReasoning('low'), true); });
  let sending, saving;
  await act(() => { sending = env.chat().send('fixture', 'Hello', null); });
  assert.equal(env.pending('ai_chat_send').command.reasoningEffort, 'low');
  await act(() => { saving = env.chat().setReasoning('high'); });
  assert.equal(env.chat().settingsBusy, true);
  assert.equal(env.pending('ai_chat_reasoning'), undefined, 'the active request starts with its original effort');

  await env.reply('ai_chat_send', { taskId: 'task', conversation: conversation() });
  assert.equal(await sending, true);
  assert.equal(env.pending('ai_chat_reasoning').command.reasoningEffort, 'high');
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 1, state: { status: 'completed', text: 'The latest streamed answer', reasoningEffort: 'low' } });
  assert.ok(env.pending('ai_chat_get'));
  await env.reply('ai_chat_reasoning', conversation('high'));
  assert.equal(await saving, true);
  assert.equal(env.chat().conversation.messages.at(-1).content, 'The latest streamed answer', 'a settings snapshot cannot erase streamed text');
  assert.equal(env.chat().busy, true);
  assert.equal(env.chat().reasoningEffort, 'high');
  await env.reply('ai_chat_get', conversation('low', 'The completed answer', 'completed'));
  assert.equal(env.chat().reasoningEffort, 'high', 'a late completion snapshot cannot restore the previous effort');
  assert.equal(env.chat().conversation.messages.at(-1).reasoningEffort, 'low');
  assert.equal(env.chat().conversation.messages.at(-1).content, 'The completed answer');
  assert.equal(env.chat().conversation.messages.at(-1).status, 'completed');
  assert.equal(env.chat().busy, false);
  assert.equal(env.calls.some(entry => entry.command.op === 'ai_cancel'), false);

  await act(() => { sending = env.chat().send('fixture', 'Next turn', null); });
  assert.equal(env.pending('ai_chat_send').command.reasoningEffort, 'high');
  await env.reply('ai_chat_send', { taskId: 'next-task', conversation: conversation('high') });
  assert.equal(await sending, true);
  await act(() => { saving = env.chat().setReasoning('medium'); });
  await act(() => env.chat().stop());
  await env.reply('ai_chat_reasoning', conversation('medium'));
  assert.equal(await saving, true);
  assert.equal(env.chat().reasoningEffort, 'medium', 'stopping a response does not discard a pending preference change');
  assert.equal(env.chat().conversation.messages.at(-1).status, 'stopped');
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 1, state: { status: 'completed', text: 'Late text', reasoningEffort: 'high' } });
  assert.equal(env.chat().busy, false);
  assert.equal(env.chat().conversation.messages.at(-1).status, 'stopped');
});

test('backtracking sends the selected prompt identity and rejects a late update from the abandoned reply', async t => {
  const env = await harness();
  t.after(env.unmount);
  let opening, sending;
  const original = conversation();
  original.messages.push({ id: 'later-user', role: 'user', content: 'Later question', status: 'completed' }, { id: 'later-reply', role: 'assistant', content: 'Abandoned answer', status: 'streaming' });
  await act(() => { opening = env.chat().open('chat'); });
  await env.reply('ai_chat_get', original);
  await opening;
  assert.equal(env.chat().busy, true);
  await act(() => env.chat().stop());
  await act(() => { sending = env.chat().send('fixture', 'Revised question', null, undefined, false, undefined, 'user'); });
  assert.equal(env.pending('ai_chat_send').command.editMessageId, 'user');
  assert.equal(env.pending('ai_chat_send').command.chatId, 'chat');
  assert.equal(env.chat().conversation.messages.length, 4, 'the draft does not truncate durable history before acceptance');
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 99, state: { status: 'completed', text: 'Late abandoned answer' } });
  const edited = conversation();
  edited.activeTask = 'edited-task';
  edited.messages[0].content = 'Revised question';
  edited.messages[1].id = 'edited-reply';
  await env.reply('ai_chat_send', { taskId: 'edited-task', conversation: edited });
  assert.equal(await sending, true);
  assert.deepEqual(env.chat().conversation.messages.map(message => message.id), ['user', 'edited-reply']);
  assert.equal(env.chat().conversation.messages[0].content, 'Revised question');
  assert.equal(env.chat().busy, true, 'abandoned completion cannot stop the replacement reply');
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 1, state: { status: 'completed', text: 'Replacement answer' } });
  const saved = { ...edited, activeTask: null, messages: edited.messages.map(message => message.id === 'edited-reply' ? { ...message, content: 'Replacement answer', status: 'completed' } : message) };
  await env.reply('ai_chat_get', saved);
  assert.deepEqual(env.chat().conversation, saved);
  assert.equal(env.chat().busy, false);
});

test('regeneration forwards the selected response identity and waits for acceptance before replacing history', async t => {
  const env = await harness();
  t.after(env.unmount);
  const original = conversation('low', 'Original answer', 'completed');
  original.messages.push({ id: 'later-user', role: 'user', content: 'Later question', status: 'completed' }, { id: 'later-reply', role: 'assistant', content: 'Later answer', status: 'completed' });
  let opening, sending;
  await act(() => { opening = env.chat().open('chat'); });
  await env.reply('ai_chat_get', original);
  await opening;
  await act(() => { sending = env.chat().send('fixture', '', null, undefined, true, undefined, undefined, 'reply'); });
  const command = env.pending('ai_chat_send').command;
  assert.equal(command.chatId, 'chat');
  assert.equal(command.retry, true);
  assert.equal(command.retryMessageId, 'reply');
  assert.equal(command.editMessageId, undefined);
  assert.deepEqual(env.chat().conversation.messages, original.messages);
  assert.equal(await env.chat().send('fixture', '', null, undefined, true, undefined, undefined, 'later-reply'), false, 'a second tap cannot start another request');
  const regenerated = conversation();
  regenerated.messages[1].id = 'replacement';
  await env.reply('ai_chat_send', { taskId: 'task', conversation: regenerated });
  assert.equal(await sending, true);
  assert.deepEqual(env.chat().conversation.messages.map(message => message.id), ['user', 'replacement']);
  await env.reply('ai_chat_poll', { kind: 'snapshot', revision: 1, state: { status: 'completed', text: 'Regenerated answer' } });
  const saved = { ...regenerated, activeTask: null, messages: regenerated.messages.map(message => message.id === 'replacement' ? { ...message, content: 'Regenerated answer', status: 'completed' } : message) };
  await env.reply('ai_chat_get', saved);
  assert.deepEqual(env.chat().conversation, saved);
  assert.equal(env.chat().busy, false);
});
