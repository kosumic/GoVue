import { useEffect, useRef, useState } from 'react';
import { aiRequest } from '../../modules/lingoplay-core';
import type { MessageUsageDetails, TokenUsage } from './usage';
import type { GameAction, LearningGame } from './games';
import { applyChatUpdate, type ChatStream, type ChatUpdate } from './chatUpdates';
import type { ChatSearchHit } from './useChatFind';

export type ActivityStatus = 'preparing' | 'running' | 'completed' | 'failed' | 'stopped';
export type ChatBlock =
  | { id: string; kind: 'text'; content: string; status: ActivityStatus }
  | { id: string; kind: 'reasoning'; content: string; status: ActivityStatus }
  | { id: string; kind: 'tool'; callId: string; name: string; status: ActivityStatus; startedAt: number; durationMs: number; gameId?: string; title?: string; error?: string };

export type ChatTranscript = { videoId: string; trackId: string; title: string; language: string };
export type ChatMessage = { id: string; role: 'user' | 'assistant'; content: string; games?: LearningGame[]; blocks?: ChatBlock[]; status: 'completed' | 'streaming' | 'failed' | 'stopped'; error?: string; reasoning?: string; reasoningEffort?: string; usage?: TokenUsage; usageDetails?: MessageUsageDetails; model?: string; modelName?: string; providerLabel?: string; completedAt?: number };
export type Conversation = { id: string; videoId?: string | null; title: string; pinned?: boolean; archived?: boolean; providerId: string; providerLabel: string; model: string; createdAt: number; updatedAt: number; transcript: ChatTranscript | null; messages: ChatMessage[]; activeTask: string | null; reasoningEffort?: string | null };
export type SavedChat = Pick<Conversation, 'id' | 'videoId' | 'title' | 'pinned' | 'archived' | 'providerId' | 'providerLabel' | 'model' | 'updatedAt'> & { videoTitle?: string; searchMatch?: ChatSearchHit | null };
type Started = { taskId: string; conversation: Conversation };

export function useChats(visible: boolean, contextId: string) {
  const [conversation, setConversation] = useState<Conversation | null>(null);
  const [history, setHistory] = useState<SavedChat[]>([]);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState('');
  const [draftReasoning, setDraftReasoning] = useState<string | null>(null);
  const reasoningDraft = useRef<string | null>(null);
  const [settingsBusy, setSettingsBusy] = useState(false);
  const settingsPending = useRef(false);
  const reasoningRevision = useRef(0);
  const starting = useRef<Promise<Started> | null>(null);
  const current = useRef<Conversation | null>(null);
  const epoch = useRef(0);
  const task = useRef<string | null>(null);
  const sending = useRef(false);
  const cancellation = useRef<Promise<unknown> | null>(null);
  const historyEpoch = useRef(0);
  const previousContext = useRef(contextId);
  const mounted = useRef(true);
  const gameWrites = useRef(Promise.resolve());
  const gameFeedback = useRef<{ chatId: string; gameId: string }[]>([]);
  const [feedbackVersion, setFeedbackVersion] = useState(0);

  function update(chat: Conversation | null) {
    const previous = current.current;
    if (chat && previous?.id === chat.id) {
      chat = { ...chat, messages: chat.messages.map(message => {
        if (!message.games) return message;
        const old = previous.messages.find(m => m.id === message.id);
        const games = message.games.map(game => {
          const saved = old?.games?.find(g => g.id === game.id);
          return saved && saved.revision > game.revision ? saved : game;
        });
        return games.some((game, index) => game !== message.games![index]) ? { ...message, games } : message;
      }) };
    }
    current.current = chat; setConversation(chat);
  }
  async function refresh() {
    const version = ++historyEpoch.current;
    const result = await aiRequest<{ chats: SavedChat[] }>({ op: 'ai_chats' });
    if (mounted.current && version === historyEpoch.current) setHistory(result.chats);
  }
  function cancel(clearFeedback: boolean) {
    if (clearFeedback) gameFeedback.current = [];
    ++epoch.current;
    const id = task.current; task.current = null;
    starting.current = null;
    sending.current = false; setBusy(false); setLoading(false);
    const chat = current.current;
    if (chat) update({ ...chat, activeTask: null, messages: chat.messages.map(m => m.status === 'streaming' ? { ...m, status: 'stopped' } : m) });
    if (id) {
      const pending = aiRequest({ op: 'ai_cancel', taskId: id }).then(() => aiRequest({ op: 'ai_forget', taskId: id })).then(refresh).catch(e => { if (mounted.current) setError(e.message); });
      cancellation.current = pending;
      void pending.finally(() => { if (cancellation.current === pending) cancellation.current = null; });
    }
  }
  function stop() { cancel(true); }
  async function watch(started: Started, version: number) {
    let stream: ChatStream | null = null;
    try {
      while (mounted.current && version === epoch.current) {
        const event = await aiRequest<ChatUpdate>({ op: 'ai_chat_poll', taskId: started.taskId, ...(stream ? { afterRevision: stream.revision } : {}) });
        if (!mounted.current || version !== epoch.current) return;
        let next = applyChatUpdate(stream, event);
        if (!next) {
          const snapshot = await aiRequest<ChatUpdate>({ op: 'ai_chat_poll', taskId: started.taskId });
          if (!mounted.current || version !== epoch.current) return;
          next = applyChatUpdate(null, snapshot);
          if (!next) throw Error('Could not resync this response.');
        }
        const changed = next !== stream;
        stream = next;
        const state = stream.state;
        const chat = current.current;
        if (changed && chat?.id === started.conversation.id) {
          const reply = started.conversation.messages.at(-1)?.id;
          update({ ...chat, messages: chat.messages.map(m => m.id === reply ? { ...m, content: state.text, games: state.games ?? m.games, blocks: state.blocks ?? m.blocks, reasoning: state.reasoning, reasoningEffort: state.reasoningEffort, usage: state.usage, usageDetails: state.usageDetails, model: state.model, providerLabel: state.providerLabel } : m) });
        }
        if (state.status !== 'running') {
          const revision = reasoningRevision.current;
          const saved = await aiRequest<Conversation>({ op: 'ai_chat_get', chatId: started.conversation.id });
          if (mounted.current && version === epoch.current) update({ ...saved, reasoningEffort: revision === reasoningRevision.current ? saved.reasoningEffort : current.current?.reasoningEffort });
          return;
        }
        await new Promise(resolve => setTimeout(resolve, 150));
      }
    } catch (e) {
      if (mounted.current && version === epoch.current) setError(e instanceof Error ? e.message : 'Could not finish this response.');
    } finally {
      await aiRequest({ op: 'ai_forget', taskId: started.taskId }).catch(() => {});
      if (mounted.current && version === epoch.current) { task.current = null; sending.current = false; setBusy(false); }
      if (mounted.current) void refresh().catch(() => {});
    }
  }
  async function send(id: string, prompt: string, transcript: ChatTranscript | null, intent?: 'summary', retry = false, focusGameId?: string, editMessageId?: string, retryMessageId?: string) {
    if (sending.current || settingsPending.current) return false;
    cancel(!!editMessageId || !!retryMessageId); const version = epoch.current;
    sending.current = true; setBusy(true); setError('');
    let pending: Promise<Started> | null = null;
    try {
      if (cancellation.current) await cancellation.current;
      if (!mounted.current || version !== epoch.current) return false;
      pending = aiRequest<Started>({ op: 'ai_chat_send', id, prompt, transcript: transcript ? { videoId: transcript.videoId, trackId: transcript.trackId } : null, reasoningEffort: current.current ? current.current.reasoningEffort ?? null : reasoningDraft.current, ...(current.current ? { chatId: current.current.id } : {}), ...(intent ? { intent } : {}), ...(retry ? { retry: true } : {}), ...(focusGameId ? { focusGameId } : {}), ...(editMessageId ? { editMessageId } : {}), ...(retryMessageId ? { retryMessageId } : {}) });
      starting.current = pending;
      const started = await pending;
      if (starting.current === pending) starting.current = null;
      if (!mounted.current || version !== epoch.current) {
        await aiRequest({ op: 'ai_cancel', taskId: started.taskId }).catch(() => {});
        await aiRequest({ op: 'ai_forget', taskId: started.taskId }).catch(() => {});
        return false;
      }
      task.current = started.taskId; update(started.conversation);
      void watch(started, version);
      return true;
    } catch (e) {
      if (mounted.current && version === epoch.current) { sending.current = false; setBusy(false); setError(e instanceof Error ? e.message : 'Could not send this message.'); }
      return false;
    } finally { if (starting.current === pending) starting.current = null; }
  }
  const gameAction: GameAction = (messageId, gameId, action, input, eventId) => {
    const owner = current.current?.id;
    const work = gameWrites.current.catch(() => {}).then(async () => {
      const chat = current.current;
      if (!mounted.current || !owner || chat?.id !== owner) throw Error('This game is no longer open.');
      const game = chat.messages.find(m => m.id === messageId)?.games?.find(g => g.id === gameId);
      if (!game) throw Error('This game is no longer available.');
      const result = await aiRequest<{ game: LearningGame; notify: boolean }>({ op: 'ai_game_action', chatId: owner, messageId, gameId, action, input, eventId, revision: game.revision });
      if (!mounted.current || current.current?.id !== owner) throw Error('This game is no longer open.');
      if (!current.current.messages.some(message => message.id === messageId && message.games?.some(game => game.id === gameId))) throw Error('This game is no longer available.');
      update({ ...current.current, messages: current.current.messages.map(m => m.id === messageId ? { ...m, games: m.games?.map(g => g.id === gameId ? result.game : g) } : m) });
      if (result.notify) {
        gameFeedback.current.push({ chatId: owner, gameId });
        setFeedbackVersion(value => value + 1);
      }
      return result.game;
    });
    gameWrites.current = work.then(() => {}, () => {});
    return work;
  };
  useEffect(() => {
    if (!visible || busy || settingsBusy || sending.current || !ready || !current.current) return;
    const chat = current.current;
    const pending = gameFeedback.current.filter(item => item.chatId === chat.id);
    gameFeedback.current = [];
    if (!pending.length) return;
    const latest = pending.at(-1)!;
    void send(chat.providerId, 'Give me feedback on my latest interaction with the first game in the attached game activity.', chat.transcript, undefined, false, latest.gameId);
  }, [visible, busy, settingsBusy, ready, feedbackVersion]);
  async function open(id: string) {
    stop(); const version = epoch.current;
    setError(''); setLoading(true);
    try {
      const chat = await aiRequest<Conversation>({ op: 'ai_chat_get', chatId: id });
      if (!mounted.current || version !== epoch.current) return;
      update(chat);
      if (chat.activeTask) { task.current = chat.activeTask; sending.current = true; setBusy(true); void watch({ taskId: chat.activeTask, conversation: chat }, version); }
      return chat;
    } catch (e) { if (mounted.current && version === epoch.current) setError(e instanceof Error ? e.message : 'Could not open this chat.'); }
    finally { if (mounted.current && version === epoch.current) setLoading(false); }
  }
  async function setReasoning(effort: string) {
    if (settingsPending.current) return false;
    const version = epoch.current;
    settingsPending.current = true;
    setSettingsBusy(true); setError('');
    try {
      // Start the current request with its original effort before saving the next turn's setting.
      if (starting.current) await starting.current;
      if (!mounted.current || version !== epoch.current) return false;
      const chat = current.current;
      if (!chat) { reasoningDraft.current = effort || null; setDraftReasoning(reasoningDraft.current); return true; }
      const saved = await aiRequest<Conversation>({ op: 'ai_chat_reasoning', chatId: chat.id, reasoningEffort: effort || null });
      if (!mounted.current || current.current?.id !== chat.id) return false;
      ++reasoningRevision.current;
      // The settings response may contain older streamed text. Merge only the setting.
      update({ ...current.current, reasoningEffort: saved.reasoningEffort }); return true;
    } catch (e) { if (mounted.current && version === epoch.current) setError(e instanceof Error ? e.message : 'Could not save reasoning.'); return false; }
    finally { settingsPending.current = false; if (mounted.current) setSettingsBusy(false); }
  }
  async function openVideo(videoId: string, title?: string, transcript?: ChatTranscript) {
    stop(); const version = epoch.current;
    setError(''); setLoading(true);
    try {
      const chat = await aiRequest<Conversation>({ op: 'ai_chat_video_open', videoId, ...(title ? { title } : {}), ...(transcript ? { transcript: { videoId: transcript.videoId, trackId: transcript.trackId } } : {}) });
      if (!mounted.current || version !== epoch.current) return;
      update(chat);
      void refresh().catch(() => {});
      return chat;
    } catch (e) {
      if (mounted.current && version === epoch.current) setError(e instanceof Error ? e.message : 'Could not open this video’s chat.');
      throw e;
    } finally { if (mounted.current && version === epoch.current) setLoading(false); }
  }
  async function selectModel(id: string) {
    const chat = current.current;
    if (!chat || sending.current) return false;
    const version = epoch.current;
    try {
      const saved = await aiRequest<Conversation>({ op: 'ai_chat_model', chatId: chat.id, id });
      if (!mounted.current || version !== epoch.current || current.current?.id !== chat.id) return false;
      update(saved); return true;
    } catch (e) { if (mounted.current && version === epoch.current) setError(e instanceof Error ? e.message : 'Could not change this chat’s model.'); return false; }
  }
  function newChat() { stop(); update(null); reasoningDraft.current = null; setDraftReasoning(null); setError(''); }
  async function remove(id: string) {
    if (current.current?.id === id) newChat();
    try { await aiRequest({ op: 'ai_chat_delete', chatId: id }); await refresh(); }
    catch (e) { if (mounted.current) setError(e instanceof Error ? e.message : 'Could not delete this chat.'); }
  }
  async function change(id: string, values: { title?: string; pinned?: boolean; archived?: boolean }) {
    const saved = await aiRequest<Conversation>({ op: 'ai_chat_update', chatId: id, ...values });
    if (!mounted.current) return;
    // Keep an in-flight reply; metadata changes must not replace its newer text snapshot.
    if (current.current?.id === id) update({ ...current.current, title: saved.title, pinned: saved.pinned, archived: saved.archived });
    await refresh();
  }
  useEffect(() => {
    mounted.current = true;
    return () => { stop(); mounted.current = false; };
  }, []);
  useEffect(() => {
    let active = true;
    setReady(false);
    if (!visible) stop();
    else {
      const version = epoch.current;
      void refresh().then(async () => {
        if (!active) return;
        if (current.current && version === epoch.current) {
          const revision = reasoningRevision.current;
          const chat = await aiRequest<Conversation>({ op: 'ai_chat_get', chatId: current.current.id });
          if (active && version === epoch.current) update({ ...chat, reasoningEffort: revision === reasoningRevision.current ? chat.reasoningEffort : current.current?.reasoningEffort });
        }
        if (active) setReady(true);
      }).catch(e => { if (active) { setError(e.message); setReady(true); } });
    }
    return () => { active = false; };
  }, [visible]);
  useEffect(() => {
    if (previousContext.current === contextId) return;
    previousContext.current = contextId; stop();
  }, [contextId]);
  return { conversation, history, busy, loading, ready, error, settingsBusy, gameAction, reasoningEffort: conversation ? conversation.reasoningEffort ?? null : draftReasoning, setReasoning, send, stop, open, openVideo, selectModel, remove, change, newChat, refresh };
}
export type ChatController = ReturnType<typeof useChats>;
