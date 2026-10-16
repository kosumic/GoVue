// Bundled for the YouTube page, never evaluated in React Native or Rust.
import { Player, Platform, Log, Constants } from 'youtubei.js/web';
import { mediaToken } from './media-token';

Log.setLevel(Log.Level.NONE);
let evaluationOwner;
let evaluationSequence = 0;
const evaluations = new Map();
window.__lingoplayEvalReply = (id, values) => evaluations.get(id)?.(values);
Platform.shim.eval = data => new Promise((resolve, reject) => {
  if (!evaluationOwner) { reject(Error('No active media request.')); return; }
  const evalId = String(++evaluationSequence);
  const timer = setTimeout(() => { evaluations.delete(evalId); reject(Error('Player signature request timed out.')); }, 12000);
  evaluations.set(evalId, values => {
    clearTimeout(timer); evaluations.delete(evalId);
    if (values) resolve(values); else reject(Error('Could not solve the YouTube player signature.'));
  });
  window.ReactNativeWebView?.postMessage(JSON.stringify({type:'mediaEval', ...evaluationOwner, evalId, code:data.output}));
});
let current;
let currentId;
const pageFetch = async (input, options) => {
  const url = new URL(typeof input === 'string' ? input : input.url || String(input), location.href);
  if (!['www.youtube.com', 'm.youtube.com', 'youtube.com'].includes(url.hostname) || url.protocol !== 'https:') throw Error('Unsupported player script origin.');
  url.host = location.host;
  if (url.pathname.includes('/s/player/')) {
    const actual = [...document.querySelectorAll('script[src]')].map(s => new URL(s.src, location.href)).find(s => s.pathname.includes('/s/player/') && s.pathname.endsWith('/base.js'));
    if (actual && ['www.youtube.com','m.youtube.com','youtube.com'].includes(actual.hostname)) { url.pathname = actual.pathname; }
  }
  return fetch(url.href, { ...options, headers: {}, credentials: 'same-origin' });
};

window.__lingoplayDecode = async (formats, videoId, requestId) => {
  evaluationOwner = {videoId, requestId};
  const script = [...document.querySelectorAll('script[src]')].map(s => s.src).find(s => /\/s\/player\//.test(s)) || window.ytcfg?.get?.('PLAYER_JS_URL') || '';
  const playerId = script.match(/\/s\/player\/([\w-]+)\//)?.[1];
  if (!current || currentId !== playerId) {
    currentId = playerId;
    current = Player.create(undefined, pageFetch, undefined, playerId).catch(error => { current = undefined; throw error; });
  }
  const player = await current;
  const token = await mediaToken(videoId);
  const nsig = new Map();
  const result = [];
  let failure;
  for (const format of formats) {
    if (!/^(?:audio\/mp4|video\/mp4)/.test(format.mimeType || '')) continue;
    if (!format.url && !format.signatureCipher && !format.cipher) continue;
    try {
      const url = new URL(await player.decipher(format.url, format.signatureCipher, format.cipher, nsig));
      const client = url.searchParams.get('c');
      if (client !== 'VISIONOS') url.searchParams.set('pot', token);
      const n = url.searchParams.get('n') || '';
      if (n.startsWith('enhanced_except_') || n.startsWith('_w8_')) throw Error('The player throttle signature could not be solved.');
      result.push({ ...format, url: url.href, userAgent: client === 'VISIONOS' ? Constants.CLIENTS.VISIONOS.USER_AGENT : undefined });
    } catch (error) { failure = error; }
  }
  if (!result.length && failure) throw failure;
  return result;
};

window.__lingoplayMediaInfo = async videoId => {
  const client = Constants.CLIENTS.VISIONOS;
  const pageContext = window.ytcfg?.get?.('INNERTUBE_CONTEXT') || {};
  const response = await fetch('/youtubei/v1/player?prettyPrint=false', {
    method: 'POST', credentials: 'omit', headers: { 'Content-Type': 'application/json', 'X-Youtube-Client-Name': '101', 'X-Youtube-Client-Version': client.VERSION },
    body: JSON.stringify({ videoId, context: { ...pageContext, client: {
      ...pageContext.client,
      clientName: 'VISIONOS', clientVersion: client.VERSION, deviceMake: client.DEVICE_MAKE, deviceModel: client.DEVICE_MODEL,
      osName: client.OS_NAME, osVersion: client.OS_VERSION, hl: pageContext.client?.hl || 'en', gl: pageContext.client?.gl || 'US',
    } }, contentCheckOk: true, racyCheckOk: true }),
  });
  if (!response.ok) return null;
  const data = await response.json();
  if (data.videoDetails?.videoId !== videoId || data.playabilityStatus?.status !== 'OK') return null;
  return data.streamingData || null;
};
