import { coreRequest } from '../../modules/lingoplay-core';

export type MediaSettings = { adBlock: boolean; sponsorBlock: boolean; backgroundPlayback: boolean; autoPip: boolean; categories: string[] };
export const defaultMediaSettings: MediaSettings = { adBlock: true, sponsorBlock: true, backgroundPlayback: true, autoPip: false, categories: ['sponsor', 'selfpromo', 'interaction'] };
export type MediaStream = { itag: number; url: string; userAgent: string; kind: 'audio' | 'video' | 'combined'; mimeType: string; height: number; bitrate: number; bytes: number; label: string };
export type MediaCatalog = { videoId: string; origin: string; title: string; author: string; duration: number; position: number; live: boolean; streams: MediaStream[]; hlsUrl: string | null };
export type DownloadPlan = { parts: MediaStream[]; extension: 'mp4' | 'm4a'; mimeType: string; label: string };
export type SponsorSegment = { start: number; end: number; category: string };

export const mediaSettings = (settings?: Partial<MediaSettings>) => coreRequest<MediaSettings>({ op: 'media_settings', settings });
export const acceptMedia = (raw: string, origin: string, requestId: string, videoId: string) => coreRequest<MediaCatalog>({ op: 'media_catalog', raw, origin, requestId, videoId });
export const planDownload = (catalog: MediaCatalog, itag: number) => coreRequest<DownloadPlan>({ op: 'media_plan', catalog, itag });
export async function fetchSponsors(videoId: string, signal: AbortSignal) {
  const url = await coreRequest<string>({ op: 'sponsor_request', videoId });
  const response = await fetch(url, { signal });
  if (response.status === 404) return [];
  if (!response.ok) throw Error('SponsorBlock is temporarily unavailable.');
  return coreRequest<SponsorSegment[]>({ op: 'sponsor_parse', videoId, raw: await response.text() });
}
