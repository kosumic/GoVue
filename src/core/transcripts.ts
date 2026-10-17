import { coreRequest } from '../../modules/lingoplay-core';
import type { Segment, Transcript } from '../youtube/transcript';
import { extract } from './extraction';

export function extractTranscript(videoId: string, trackId = 'default', signal?: AbortSignal, fresh = false, audioKey = '') {
  return extract<Transcript>('transcript', videoId, { trackId, signal, fresh, audioKey });
}

export function restoreTranscript(videoId: string, trackId?: string, audioKey = '') {
  return coreRequest<Transcript | null>({ op: 'restore', videoId, trackId, audioKey });
}

export type TranscriptRow = { start: number; lines: { track: number; index: number }[] };
export type TranscriptReader = { rows: TranscriptRow[]; total: number };

export function readTranscripts(data: Transcript, comparison: Transcript | null, query: string) {
  return coreRequest<TranscriptReader>({ op: 'transcript_reader', videoId: data.videoId, trackId: data.trackId, comparisonTrackId: comparison?.trackId, query });
}

export function selectTranscriptTracks(data: Transcript, trackIds: string[], toggleId: string) {
  return coreRequest<string[]>({ op: 'transcript_select', videoId: data.videoId, trackId: data.trackId, trackIds, toggleId });
}

export async function searchTranscript(data: Transcript, query: string): Promise<Segment[]> {
  const indices = await coreRequest<number[]>({ op: 'search', videoId: data.videoId, trackId: data.trackId, query });
  return indices.map(index => data.segments[index]).filter((segment): segment is Segment => !!segment);
}

export function exportTranscript(data: Transcript, comparison?: Transcript | null) {
  return coreRequest<string>({ op: comparison ? 'transcript_export' : 'export', videoId: data.videoId, trackId: data.trackId, comparisonTrackId: comparison?.trackId });
}

export function advanceTranscriptPlayback(data: Transcript, cursor: unknown, sample: {
  position: number | null; playing: boolean; seeking: boolean; rate: number; elapsed: number;
}) {
  return coreRequest<{ pause: boolean; cursor: unknown }>({ op: 'transcript_playback', videoId: data.videoId, trackId: data.trackId, cursor, ...sample });
}
