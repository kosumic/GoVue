import { BotGuardClient, getChallenge } from 'bgutils-js/botguard';
import { WebPoMinter } from 'bgutils-js/webpo';
import { getHeaders } from 'bgutils-js/utils';
import { pageTrust } from './page-trust';

const key = 'O43z0dpjhgX20SCx4KAo';
let cached;
let expires = 0;
let scope = '';
async function jsonRequest(path, body, headers = { 'Content-Type': 'application/json' }) {
  const response = await fetch(new URL(path, location.origin), { method: 'POST', credentials: 'same-origin', headers, body: JSON.stringify(body) });
  if (!response.ok) throw Error(`YouTube media verification failed (${response.status}).`);
  return response.json();
}
async function createMinter() {
  const context = window.ytcfg?.get?.('INNERTUBE_CONTEXT');
  let challenge;
  try {
    challenge = (await jsonRequest('/youtubei/v1/att/get?prettyPrint=false', { context, engagementType: 'ENGAGEMENT_TYPE_UNBOUND' })).bgChallenge;
  } catch {}
  if (!challenge?.program) challenge = await getChallenge({ requestKey: key, useYouTubeAPI: true, fetchFunction: (input, options) => {
    const url = new URL(String(input)); url.host = location.host;
    return fetch(url.href, { ...options, credentials: 'same-origin' });
  } });
  if (!challenge?.program || !challenge.globalName) throw Error('YouTube did not provide a media verification challenge.');
  if (!window[challenge.globalName]) {
    const raw = challenge.interpreterUrl?.privateDoNotAccessOrElseTrustedResourceUrlWrappedValue;
    const url = new URL(raw || '', location.origin);
    if (url.protocol !== 'https:' || !['www.youtube.com','m.youtube.com','www.google.com','www.gstatic.com'].includes(url.hostname)) throw Error('Unsupported media verification script.');
    await new Promise((resolve, reject) => {
      const script = document.createElement('script');
      const timer = setTimeout(() => { script.remove(); reject(Error('Media verification script timed out.')); }, 8000);
      script.src = pageTrust ? pageTrust.createScriptURL(url.href) : url.href;
      const nonce = document.querySelector('script[nonce]')?.nonce;
      if (nonce) script.nonce = nonce;
      script.onload = () => { clearTimeout(timer); resolve(); };
      script.onerror = () => { clearTimeout(timer); reject(Error('Could not load media verification.')); };
      document.head.appendChild(script);
    });
  }
  const client = await BotGuardClient.create({ program: challenge.program, globalName: challenge.globalName, globalObject: window });
  const webPoSignalOutput = [];
  const snapshot = await client.snapshot({ webPoSignalOutput });
  const integrity = await jsonRequest('/api/jnn/v1/GenerateIT', [key, snapshot], getHeaders());
  if (!integrity[0] || !(integrity[1] > 0)) throw Error('YouTube did not verify this media session.');
  expires = Date.now() + Math.max(1000, integrity[1] * 1000 - 60000);
  const minter = await WebPoMinter.create({ integrityToken: integrity[0], estimatedTtlSecs: integrity[1], mintRefreshThreshold: integrity[2], websafeFallbackToken: integrity[3] }, webPoSignalOutput);
  return minter;
}
export async function mediaToken(videoId) {
  const identity = window.ytcfg?.get?.('DATASYNC_ID') || window.ytcfg?.get?.('VISITOR_DATA') || '';
  if (!cached || scope !== identity || Date.now() >= expires) {
    scope = identity; expires = Date.now() + 30000;
    cached = createMinter().catch(error => { cached = undefined; throw error; });
  }
  return (await cached).mintAsWebsafeString(videoId);
}
