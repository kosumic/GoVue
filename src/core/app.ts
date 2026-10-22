import { coreRequest } from '../../modules/govue-core';

export const REPOSITORY_URL = 'https://github.com/kosumic/GoVue';
export const LATEST_RELEASE_URL = `${REPOSITORY_URL}/releases/latest`;
export const DEVELOPER_URL = 'https://x.com/Kosumi1989';
export const WHIP_DOWNLOAD_URL = 'https://kosumic.github.io/whip/';

export type AppUpdate = { latestVersion: string; updateAvailable: boolean };
export type LicenseNotice = {
  id: string; title: string; license: string;
  category: 'featured' | 'npm' | 'cargo';
  sourceUrl: string; attribution: string;
};

// HTTPS is a platform effect; release validation and version policy stay in Rust.
export async function checkAppUpdate(installedVersion: string, signal: AbortSignal): Promise<AppUpdate> {
  const response = await fetch('https://api.github.com/repos/kosumic/GoVue/releases/latest', {
    headers: { Accept: 'application/vnd.github+json' },
    cache: 'no-store',
    credentials: 'omit',
    signal,
  });
  if (response.status === 404) throw Error('No stable release has been published yet.');
  if (response.status === 403 || response.status === 429) throw Error('GitHub is limiting update checks. Please try again later.');
  if (!response.ok) throw Error('Could not check for updates. Please try again.');
  const raw = await response.text();
  if (raw.length > 256_000) throw Error('GitHub returned an unreadable release.');
  return coreRequest<AppUpdate>({ op: 'app_update', installedVersion, release: JSON.parse(raw) });
}
