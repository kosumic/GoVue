import { aiRequest } from '../../modules/govue-core';

export type GameScope = { gameId: string } & ({ practice: true } | { chatId: string; messageId: string });
export type TranscriptScope = { chatId: string; videoId: string; trackId: string };
export type WordScope = { videoId: string; trackId: string; segmentIndex: number; tokenIndex: number; word: string };
export type CaptionScope = GameScope | TranscriptScope | WordScope;
export type Clip = { needsMedia: false; uri: string; token: string; start: number; end: number };
export type MissingClip = { needsMedia: true; videoId: string };

export function audioCommand(scope: CaptionScope, cueId: string) {
  const op = 'word' in scope ? 'ai_word_audio' : 'gameId' in scope
    ? 'practice' in scope ? 'ai_practice_audio' : 'ai_game_audio' : 'ai_chat_audio';
  return { op, ...scope, cueId };
}

let sequence = 0;
async function prepareAudio(scope: CaptionScope, cueId: string, signal: AbortSignal, prefetch = false): Promise<Clip> {
  if (signal.aborted) throw Error('Audio preparation cancelled.');
  const requestId = `audio:${Date.now()}:${++sequence}`;
  const cancel = () => { void aiRequest({ op: 'ai_caption_audio_cancel', requestId }).catch(() => {}); };
  signal.addEventListener('abort', cancel, { once: true });
  try {
    return await aiRequest<Clip>({ op: 'ai_caption_audio', requestId,
      command: { ...audioCommand(scope, cueId), ...(prefetch ? { prefetch: true } : {}) } });
  } finally { signal.removeEventListener('abort', cancel); }
}
export function requestCaptionAudio(scope: CaptionScope, cueId: string, signal: AbortSignal): Promise<Clip> {
  return prepareAudio(scope, cueId, signal);
}

// Controllers adapt view lifetime to Rust; preparation deduplication and retries are shared.
export function captionAudioPreparation(scope: CaptionScope) {
  const controllers = new Set<AbortController>();
  return {
    async prepare(cueId: string): Promise<void> {
      const controller = new AbortController();
      controllers.add(controller);
      try { await prepareAudio(scope, cueId, controller.signal, true); }
      finally { controllers.delete(controller); }
    },
    cancel() { for (const controller of controllers) controller.abort(); controllers.clear(); },
  };
}
