import { browserRequest } from '../../modules/govue-core';

export const YOUTUBE_HOME = 'https://www.youtube.com/';

export const browserAddress = (input: string) => browserRequest<string>({ op: 'browser_input', input });

type Identity = { youtube: boolean; videoId: string | null };
const identities = new Map<string, Identity>();
function identity(url: string) {
  if (url.length > 8192) return { youtube: false, videoId: null };
  let result = identities.get(url);
  if (!result) {
    try { result = browserRequest<Identity>({ op: 'browser_identity', url }); }
    catch { return { youtube: false, videoId: null }; }
    if (identities.size >= 64) identities.delete(identities.keys().next().value!);
    identities.set(url, result);
  }
  return result;
}
export const isYouTubeUrl = (url: string) => identity(url).youtube;
export const youtubeVideoId = (url: string) => identity(url).videoId;
