import { useEffect, useState } from 'react';
import { coreRequest } from '../../modules/govue-core';
import type { MediaChapter } from '../core/media';

const emptyChapters: MediaChapter[] = [];

/** Rust owns chapter selection; both the controls and sheet show the same result. */
export function usePlayerChapter(
  videoId: string,
  chapters: MediaChapter[] = emptyChapters,
  position: number,
  enabled: boolean,
) {
  const [selection, setSelection] = useState<{
    videoId: string;
    chapters: MediaChapter[];
    index: number | null;
  } | null>(null);
  useEffect(() => {
    if (!enabled || !chapters.length) return;
    let active = true;
    void coreRequest<number | null>({
      op: 'player_chapter',
      starts: chapters.map((chapter) => chapter.start),
      position,
    })
      .then((index) => {
        if (active) setSelection({ videoId, chapters, index });
      })
      .catch(() => {
        if (active) setSelection(null);
      });
    return () => {
      active = false;
    };
  }, [enabled, videoId, chapters, position]);
  return selection?.videoId === videoId && selection.chapters === chapters
    ? selection.index
    : null;
}
