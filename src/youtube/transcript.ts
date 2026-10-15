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

export function timestamp(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remainder = String(total % 60).padStart(2, '0');
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${remainder}` : `${minutes}:${remainder}`;
}
