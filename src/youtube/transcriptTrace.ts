import { transcriptTrace } from '../../modules/govue-core';

export const TRANSCRIPT_TRACING = process.env.EXPO_PUBLIC_GOVUE_TRACE_TRANSCRIPT === '1';

const stages = new Set([
  'request', 'cache.restore', 'bridge.extract', 'core.accept', 'reader.commit',
  'page.extract', 'page.player_captions', 'page.transcript_control',
  'page.language', 'page.api.next', 'page.api.get_transcript', 'page.caption_url',
]);
let nextCookie = 0x4c500000;

export function createTranscriptTrace(requestId: string) {
  const active = new Map<string, number>();
  let finished = false;
  const event = (stage: string, begin: boolean) => {
    if (!TRANSCRIPT_TRACING || finished || !stages.has(stage)) return;
    if (begin) {
      if (active.has(stage)) return;
      const cookie = ++nextCookie;
      if (transcriptTrace(requestId, stage, cookie, true)) active.set(stage, cookie);
    } else {
      const cookie = active.get(stage);
      if (cookie === undefined) return;
      transcriptTrace(requestId, stage, cookie, false);
      active.delete(stage);
    }
  };
  const finish = () => {
    for (const stage of [...active.keys()].reverse()) event(stage, false);
    finished = true;
  };
  event('request', true);
  return { event, finish };
}

export type TranscriptTrace = ReturnType<typeof createTranscriptTrace>;
