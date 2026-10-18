import { useEffect, useRef, useState } from 'react';
import { aiRequest } from '../../modules/lingoplay-core';
import type { WordSelection } from '../core/vocabulary';

export type WordExample = { sentence: string; sourceLanguage: string; sourceLangCode: string; translations: { languageCode: string; language: string; text: string }[] };
export type WordGrammar = { baseForm: string; explanation: string; language: string };
type Task<T> = { status: 'running' | 'completed' | 'failed' | 'cancelled'; result?: T; error?: string };
type Request = { active: boolean; taskId?: string };

async function release(request: Request) {
  request.active = false;
  if (request.taskId) {
    await aiRequest({ op: 'ai_cancel', taskId: request.taskId }).catch(() => {});
    await aiRequest({ op: 'ai_forget', taskId: request.taskId }).catch(() => {});
  }
}

export function useWordAssistance<T extends WordExample | WordGrammar>(selection: WordSelection, kind: 'example' | 'grammar') {
  const [result, setResult] = useState<T | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const current = useRef<Request | null>(null);
  useEffect(() => {
    setResult(null); setBusy(false); setError('');
    return () => {
      if (current.current) void release(current.current);
      current.current = null;
    };
  }, [selection, kind]);

  async function generate() {
    if (current.current) void release(current.current);
    const request: Request = { active: true };
    current.current = request;
    setBusy(true); setError('');
    try {
      const { source, segmentIndex, tokenIndex, word } = selection;
      const started = await aiRequest<{ taskId: string }>({ op: 'ai_word_assistance', kind, videoId: source.videoId, trackId: source.trackId, segmentIndex, tokenIndex, word });
      request.taskId = started.taskId;
      if (!request.active) return;
      while (request.active) {
        const task = await aiRequest<Task<T>>({ op: 'ai_poll', taskId: started.taskId });
        if (!request.active) return;
        if (task.status === 'failed') throw Error(task.error || 'Could not generate this answer.');
        if (task.status === 'cancelled') return;
        if (task.status === 'completed') {
          if (!task.result) throw Error('The AI returned an empty answer. Try again.');
          setResult(task.result);
          return;
        }
        await new Promise(resolve => setTimeout(resolve, 250));
      }
    } catch (failure) {
      if (request.active) setError(failure instanceof Error ? failure.message : 'Could not generate this answer.');
    } finally {
      if (request.active) { setBusy(false); current.current = null; }
      await release(request);
    }
  }
  return { generate, result, busy, error };
}
