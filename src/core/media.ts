import defaults from '../../core/src/player/defaults.json';
import {
  coreRequest,
  supportedMediaFormats,
} from '../../modules/govue-core';
import { Platform } from 'react-native';
import { extract } from './extraction';
import type { CaptionTrack } from '../youtube/transcript';

export type GestureKey =
  `gesture${'Tap' | 'DoubleTap' | 'LongPress' | 'Brightness' | 'Volume' | 'Seek' | 'Fullscreen'}${'Windowed' | 'Fullscreen'}`;
export type SubtitleStyle = {
  scale: number;
  vertical: number;
  horizontal: number;
  background: 'none' | 'semi' | 'opaque';
  bold: boolean;
  color: string;
};
export type MediaSettings = {
  adBlock: boolean;
  sponsorBlock: boolean;
  backgroundPlayback: boolean;
  autoPip: boolean;
  pauseAfterLine: boolean;
  playbackSpeed: number;
  qualityHeight: number;
  qualityFps: number;
  categories: string[];
  rememberPosition: boolean;
  rememberQuality: boolean;
  rememberSpeed: boolean;
  rememberResize: boolean;
  useOriginalTitle: boolean;
  showDislikes: boolean;
  showLikes: boolean;
  hideShorts: boolean;
  miniPlayer: boolean;
  sponsorCountdown: boolean;
  hapticStrength: number;
  resizeMode: 'fit' | 'fill' | 'zoom' | 'width' | 'height';
  audioKey: string;
  subtitleKey: string;
  subtitleStyle: SubtitleStyle;
} & Record<GestureKey, boolean>;
export const defaultMediaSettings = defaults as MediaSettings;
export type MediaByteRange = { start: number; end: number };
export type MediaStream = {
  itag: number;
  key?: string;
  fps?: number;
  audioKey?: string;
  audioLabel?: string;
  audioDefault?: boolean;
  audioOriginal?: boolean;
  url: string;
  userAgent: string;
  kind: 'audio' | 'video' | 'combined';
  mimeType: string;
  height: number;
  width?: number;
  initRange?: MediaByteRange | null;
  indexRange?: MediaByteRange | null;
  bitrate: number;
  bytes: number;
  label: string;
};
export type MediaChapter = {
  start: number;
  title: string;
  previewUrl?: string | null;
};
export type MediaCatalog = {
  videoId: string;
  origin: string;
  title: string;
  author: string;
  duration: number;
  position: number;
  live: boolean;
  streams: MediaStream[];
  hlsUrl: string | null;
  hlsUserAgent?: string;
  subtitles?: CaptionTrack[];
  chapters?: MediaChapter[];
  audioTracks?: { key: string; label: string }[];
};
export type DownloadPlan = {
  parts: MediaStream[];
  extension: 'mp4' | 'm4a';
  mimeType: string;
  label: string;
};
export type SponsorSegment = { start: number; end: number; category: string };
export type PlaybackQuality = { height: number; fps?: number; label: string };
export type PlaybackSource = {
  uri: string;
  contentType: 'progressive' | 'hls' | 'dash';
  qualityHeight: number;
  qualityFps?: number;
  headers?: Record<string, string>;
  startupHeight?: number;
};

export const mediaSettings = (settings?: Partial<MediaSettings>) =>
  coreRequest<MediaSettings>({ op: 'media_settings', settings });
export const mediaSponsorCategory = (id: string, enabled: boolean) =>
  coreRequest<MediaSettings>({ op: 'media_settings', category: { id, enabled } });
export async function extractMedia(videoId: string, signal?: AbortSignal, fresh = false) {
  return applyMediaCapabilities(await extract<MediaCatalog>('media', videoId, { signal, fresh }));
}
async function applyMediaCapabilities(catalog: MediaCatalog) {
  const keys = await supportedMediaFormats(
    catalog.streams.map((s) => ({
      key: s.key || `${s.itag}:`,
      mimeType: s.mimeType,
      width: s.width || 0,
      height: s.height,
      fps: s.fps || 0,
    })),
  );
  return coreRequest<MediaCatalog>({ op: 'media_capabilities', catalog, keys });
}
export const planDownload = (catalog: MediaCatalog, itag: number) =>
  coreRequest<DownloadPlan>({ op: 'media_plan', catalog, itag });
export const playbackChoices = (catalog: MediaCatalog) =>
  coreRequest<PlaybackQuality[]>({ op: 'media_playback_choices', catalog });
export const playbackSpeeds = () =>
  coreRequest<number[]>({ op: 'media_speeds' });
export const preparePlayback = (
  catalog: MediaCatalog,
  height: number,
  userAgent: string,
  allowFallback = false,
  audioKey = '',
  fps = 0,
) =>
  coreRequest<PlaybackSource>({
    op: 'media_playback',
    catalog,
    height,
    userAgent,
    allowFallback,
    audioKey,
    fps,
    manifest: Platform.OS === 'android' ? 'dash' : 'hls',
  });
export async function fetchSponsors(videoId: string, signal: AbortSignal) {
  const url = await coreRequest<string>({ op: 'sponsor_request', videoId });
  const response = await fetch(url, { signal });
  if (response.status === 404) return [];
  if (!response.ok) throw Error('SponsorBlock is temporarily unavailable.');
  return coreRequest<SponsorSegment[]>({
    op: 'sponsor_parse',
    videoId,
    raw: await response.text(),
  });
}
