import { coreRequest } from '../../modules/lingoplay-core';
import type { Segment, Transcript, TranscriptReply } from '../youtube/transcript';

export function acceptTranscript(raw: string, origin: string, requestId: string, videoId: string) {
  return coreRequest<TranscriptReply>({ op: 'accept', raw, origin, requestId, videoId });
}

export function restoreTranscript(videoId: string) {
  return coreRequest<Transcript | null>({ op: 'restore', videoId });
}

export async function searchTranscript(data: Transcript, query: string): Promise<Segment[]> {
  const indices = await coreRequest<number[]>({ op: 'search', videoId: data.videoId, trackId: data.trackId, query });
  return indices.map(index => data.segments[index]).filter((segment): segment is Segment => !!segment);
}

export function exportTranscript(data: Transcript) {
  return coreRequest<string>({ op: 'export', videoId: data.videoId, trackId: data.trackId });
}
