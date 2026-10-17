import { useEffect, useRef, useState } from 'react';
import { aiRequest } from '../../modules/lingoplay-core';
import type { TokenUsage } from './usage';

export type ChatTranscript = { videoId: string; trackId: string; title: string; language: string };
export type ChatMessage = { id: string; role: 'user' | 'assistant'; content: string; status: 'completed' | 'streaming' | 'failed' | 'stopped'; error?: string; reasoning?: string; reasoningEffort?: string; usage?: TokenUsage; model?: string; providerLabel?: string };
export type Conversation = { id: string; title: string; providerId: string; providerLabel: string; model: string; createdAt: number; updatedAt: number; transcript: ChatTranscript | null; messages: ChatMessage[]; activeTask: string | null; reasoningEffort?: string | null };
export type SavedChat = Pick<Conversation, 'id' | 'title' | 'providerId' | 'providerLabel' | 'model' | 'updatedAt'> & { videoTitle?: string };
type Started = { taskId: string; conversation: Conversation };
type Progress = { status: 'running' | 'completed' | 'failed' | 'cancelled'; text: string; error?: string; reasoning?: string; reasoningEffort?: string; usage?: ChatMessage['usage']; model?: string; providerLabel?: string };

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
  const current = useRef<Conversation | null>(null);
  const epoch = useRef(0);
  const task = useRef<string | null>(null);
  const sending = useRef(false);
  const historyEpoch = useRef(0);
  const previousContext = useRef(contextId);
  const mounted = useRef(true);

  function update(chat: Conversation | null) { current.current = chat; setConversation(chat); }
  async function refresh() {
    const version = ++historyEpoch.current;
    const result = await aiRequest<{ chats: SavedChat[] }>({ op: 'ai_chats' });
    if (mounted.current && version === historyEpoch.current) setHistory(result.chats);
  }
  function stop() {
    ++epoch.current;
    const id = task.current; task.current = null;
    sending.current = false; setBusy(false); setLoading(false);
    const chat = current.current;
    if (chat) update({ ...chat, activeTask: null, messages: chat.messages.map(m => m.status === 'streaming' ? { ...m, status: 'stopped' } : m) });
    if (id) void aiRequest({ op: 'ai_cancel', taskId: id }).then(() => aiRequest({ op: 'ai_forget', taskId: id })).then(refresh).catch(e => { if (mounted.current) setError(e.message); });
  }
  async function watch(started: Started, version: number) {
    try {
      while (mounted.current && version === epoch.current) {
        const state = await aiRequest<Progress>({ op: 'ai_poll', taskId: started.taskId });
        if (!mounted.current || version !== epoch.current) return;
        const chat = current.current;
        if (chat?.id === started.conversation.id) {
          const reply = started.conversation.messages.at(-1)?.id;
          update({ ...chat, messages: chat.messages.map(m => m.id === reply ? { ...m, content: state.text, reasoning: state.reasoning, reasoningEffort: state.reasoningEffort, usage: state.usage, model: state.model, providerLabel: state.providerLabel } : m) });
        }
        if (state.status !== 'running') {
          const saved = await aiRequest<Conversation>({ op: 'ai_chat_get', chatId: started.conversation.id });
          if (mounted.current && version === epoch.current) update(saved);
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
  async function send(id: string, prompt: string, transcript: ChatTranscript | null, intent?: 'summary', retry = false) {
    if (sending.current || settingsBusy) return false;
    stop(); const version = epoch.current;
    sending.current = true; setBusy(true); setError('');
    try {
      const started = await aiRequest<Started>({ op: 'ai_chat_send', id, prompt, transcript: transcript ? { videoId: transcript.videoId, trackId: transcript.trackId } : null, reasoningEffort: current.current ? current.current.reasoningEffort ?? null : reasoningDraft.current, ...(current.current ? { chatId: current.current.id } : {}), ...(intent ? { intent } : {}), ...(retry ? { retry: true } : {}) });
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
    }
  }
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
    if (sending.current || settingsBusy) return false;
    const chat = current.current;
    if (!chat) { reasoningDraft.current = effort || null; setDraftReasoning(reasoningDraft.current); return true; }
    const version = epoch.current;
    setSettingsBusy(true); setError('');
    try {
      const saved = await aiRequest<Conversation>({ op: 'ai_chat_reasoning', chatId: chat.id, reasoningEffort: effort || null });
      if (!mounted.current || version !== epoch.current || current.current?.id !== chat.id) return false;
      update(saved); return true;
    } catch (e) { if (mounted.current && version === epoch.current) setError(e instanceof Error ? e.message : 'Could not save reasoning.'); return false; }
    finally { if (mounted.current) setSettingsBusy(false); }
  }
  function newChat() { stop(); update(null); reasoningDraft.current = null; setDraftReasoning(null); setError(''); }
  async function remove(id: string) {
    if (current.current?.id === id) newChat();
    try { await aiRequest({ op: 'ai_chat_delete', chatId: id }); await refresh(); }
    catch (e) { if (mounted.current) setError(e instanceof Error ? e.message : 'Could not delete this chat.'); }
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
          const chat = await aiRequest<Conversation>({ op: 'ai_chat_get', chatId: current.current.id });
          if (active && version === epoch.current) update(chat);
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
  return { conversation, history, busy, loading, ready, error, settingsBusy, reasoningEffort: conversation ? conversation.reasoningEffort ?? null : draftReasoning, setReasoning, send, stop, open, remove, newChat, refresh };
}
export type ChatController = ReturnType<typeof useChats>;
