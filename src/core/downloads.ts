import { coreRequest } from '../../modules/lingoplay-core';
import type { MediaCatalog, MediaStream } from './media';

export type DownloadConfig = {
  audioOnly: boolean;
  qualityHeight: number;
  qualityFps: number;
  videoKey: string;
  audioKey: string;
  audioBitrate: number;
  includeSubtitle: boolean;
  subtitleKey: string;
  includeCover: boolean;
  fileName: string;
  attachmentsOnly: boolean;
};
export type DownloadPreferences = {
  wifiOnly: boolean;
  maxConnections: number;
  defaultQuality: number;
  lastConfig: DownloadConfig;
};
export type DownloadPlan = {
  parts: MediaStream[];
  extension: 'mp4' | 'm4a';
  mimeType: string;
  label: string;
  name: string;
  bytes: number;
  coverUrl: string | null;
  subtitle: { id: string; label: string; language: string } | null;
  attachmentsOnly: boolean;
};
export type DownloadOptions = {
  qualities: { key: string; height: number; fps: number; label: string }[];
  audioTracks: { key: string; label: string }[];
  subtitles: {
    id: string;
    label: string;
    language: string;
    automatic: boolean;
  }[];
  bitrates: number[];
  preview: DownloadPlan | null;
  error: string | null;
};
export type DownloadArtifact = {
  file: string;
  mimeType: string;
  label: string;
  publishedUri?: string;
  published?: boolean;
  publishedBytes?: number;
  publishedSha256?: string;
};
export type DownloadEntry = {
  id: string;
  videoId: string;
  title: string;
  author: string;
  label: string;
  name?: string;
  itag: number;
  state:
    | 'queued'
    | 'resolving'
    | 'downloading'
    | 'paused'
    | 'combining'
    | 'complete'
    | 'failed'
    | 'cancelled'
    | 'skipped';
  received: number;
  total: number;
  file: string;
  mimeType: string;
  error?: string;
  config?: DownloadConfig;
  artifacts?: DownloadArtifact[];
  batchId?: string;
  watchPageDownloaded?: boolean;
};
export type DownloadSnapshot = {
  entries: DownloadEntry[];
  claimed: string[];
  plan: DownloadPlan | null;
  removed?: DownloadEntry[];
  added?: number;
  skipped?: number;
};
export const downloadPreferences = (settings?: Partial<DownloadPreferences>) =>
  coreRequest<DownloadPreferences>({ op: 'downloads_config', settings });
export const downloadOptions = (
  catalog: MediaCatalog,
  config: DownloadConfig,
) => coreRequest<DownloadOptions>({ op: 'downloads_options', catalog, config });
