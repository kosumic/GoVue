import { aiRequest } from '../../modules/govue-core';
import { extractMedia } from '../core/media';

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

export async function requestCaptionAudio(scope: CaptionScope, cueId: string, signal: AbortSignal): Promise<Clip> {
  const command = audioCommand(scope, cueId);
  let source = await aiRequest<Clip | MissingClip>(command);
  // Return a late route to the caller, which owns revocation even after cancellation.
  if (!source.needsMedia) return source;
  const check = () => { if (signal.aborted) throw Error('Audio playback cancelled.'); };
  check();
  const catalog = await extractMedia(source.videoId, signal);
  check();
  try { source = await aiRequest<Clip>({ ...command, catalog }); }
  catch {
    check();
    const fresh = await extractMedia(catalog.videoId, signal, true);
    check();
    source = await aiRequest<Clip>({ ...command, catalog: fresh });
  }
  return source;
}

// Preparation creates cache files, never playback routes or audio-focus changes.
// A Play tap and the visible card share a pending preparation for the same cue.
export function captionAudioPreparation(scope: CaptionScope) {
  const jobs = new Map<string, { controller: AbortController; promise: Promise<void> }>();
  function prepare(cueId: string): Promise<void> {
    const current = jobs.get(cueId);
    if (current && !current.controller.signal.aborted) return current.promise;
    const controller = new AbortController();
    const check = () => { if (controller.signal.aborted) throw Error('Audio preparation cancelled.'); };
    const command = { ...audioCommand(scope, cueId), prefetch: true };
    const job = { controller, promise: Promise.resolve() };
    job.promise = (async () => {
      const source = await aiRequest<{ needsMedia: false } | MissingClip>(command);
      check();
      if (!source.needsMedia) return;
      const catalog = await extractMedia(source.videoId, controller.signal);
      check();
      try { await aiRequest({ ...command, catalog }); }
      catch {
        check();
        const fresh = await extractMedia(source.videoId, controller.signal, true);
        check();
        await aiRequest({ ...command, catalog: fresh });
      }
      check();
    })().finally(() => { if (jobs.get(cueId) === job) jobs.delete(cueId); });
    jobs.set(cueId, job);
    return job.promise;
  }
  return { prepare, cancel() {
    for (const job of jobs.values()) job.controller.abort();
    jobs.clear();
  } };
}
