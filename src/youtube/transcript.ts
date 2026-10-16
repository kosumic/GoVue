export type CaptionTrack = { id: string; label: string; language: string; automatic: boolean };
export type Segment = { start: number; duration: number; text: string };
export type Transcript = {
  videoId: string;
  title: string;
  language: string;
  trackId: string;
  tracks: CaptionTrack[];
  segments: Segment[];
};
export type TranscriptReply = {
  type: 'transcript';
  requestId: string;
  videoId: string;
} & ({ ok: true; transcript: Transcript } | { ok: false; error: string });

// Rust supplies normalized, sorted segments; this lookup controls reader styling.
export function currentSegmentIndex(segments: Segment[], seconds: number | null): number {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return -1;
  let low = 0;
  let high = segments.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (segments[mid].start <= seconds) low = mid + 1;
    else high = mid;
  }
  const index = low - 1;
  const segment = segments[index];
  if (!segment) return -1;
  // Captions without a duration last until the next line starts.
  const end = segment.duration > 0 ? segment.start + segment.duration : segments[index + 1]?.start ?? Infinity;
  return seconds < end ? index : -1;
}

export function timestamp(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remainder = String(total % 60).padStart(2, '0');
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${remainder}` : `${minutes}:${remainder}`;
}
