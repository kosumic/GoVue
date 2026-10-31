import { useEffect, useState } from 'react';
import { useTranscriptAudio } from '../ai/useGameAudio';
import { usePlayerTranscript } from '../player/usePlayerTranscript';
import type { Transcript } from '../youtube/transcript';
import { TranscriptPanel } from './TranscriptPanel';

export function ChatTranscriptReader({ data, chatId, onClose, onBeforeAudio }: {
  data: Transcript;
  chatId: string;
  onClose: () => void;
  onBeforeAudio?: () => Promise<void> | void;
}) {
  const reader = usePlayerTranscript(data.videoId, data);
  const [line, setLine] = useState<{ source: Transcript; index: number } | null>(null);
  const audio = useTranscriptAudio({ chatId, videoId: data.videoId, trackId: line?.source.trackId ?? data.trackId }, onBeforeAudio);
  useEffect(() => {
    if (line) void audio.play(`cue-${line.index}`).catch(() => {});
  }, [line]);
  // Cached audio starts within a local fragment; word timings use the video clock.
  const segment = line?.source.segments[line.index];
  const position = segment && audio.progress?.cueId === `cue-${line?.index}` ? segment.start + audio.progress.elapsed : null;
  return <TranscriptPanel data={reader.data ?? data} comparison={reader.comparison} trackIds={reader.trackIds}
    comparisonLoading={reader.comparisonLoading} comparisonError={reader.comparisonError} retryAfterSeconds={reader.retryAfterSeconds} onSelectTrack={reader.selectTrack}
    loading={reader.loading} error={reader.error} cached={false} pauseAfterLine={false} onClose={onClose}
    position={position} audio={audio}
    onBeforeWordAudio={onBeforeAudio} onPlayLine={(index, source = data) => setLine({ source, index })} />;
}
