import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react';
import { coreRequest } from '../../modules/lingoplay-core';
import { extractMedia, type MediaCatalog } from '../core/media';
import { extractTranscript } from '../core/transcripts';
import type { DownloadConfig, DownloadPlan } from '../core/downloads';

type Resolved = { catalog: MediaCatalog; srt?: string };
export type MediaResolverHandle = {
  resolve: (videoId: string, config?: DownloadConfig, signal?: AbortSignal) => Promise<Resolved>;
};

// The resolver is an imperative UI adapter. Extraction never loads a page.
export const MediaResolver = forwardRef<MediaResolverHandle>(function MediaResolver(_, ref) {
  const active = useRef(new Set<AbortController>());
  useEffect(() => () => { for (const controller of active.current) controller.abort(); active.current.clear(); }, []);
  useImperativeHandle(ref, () => ({
    async resolve(videoId, config, signal) {
      const controller = new AbortController();
      const abort = () => controller.abort();
      signal?.addEventListener('abort', abort, { once: true });
      if (signal?.aborted) controller.abort();
      active.current.add(controller);
      const check = () => { if (controller.signal.aborted) throw Object.assign(Error('Download extraction cancelled.'), { name: 'AbortError' }); };
      try {
        const catalog = await extractMedia(videoId, controller.signal);
        check();
        if (!config?.includeSubtitle) return { catalog };
        const plan = await coreRequest<DownloadPlan>({ op: 'downloads_plan', catalog, config });
        check();
        if (!plan.subtitle) throw Error('The selected subtitles are unavailable.');
        const transcript = await extractTranscript(videoId, plan.subtitle.id, controller.signal);
        check();
        const srt = await coreRequest<string>({ op: 'downloads_subtitles', videoId, trackId: transcript.trackId });
        check();
        return { catalog, srt };
      } finally {
        signal?.removeEventListener('abort', abort);
        active.current.delete(controller);
      }
    },
  }), []);
  return null;
});
