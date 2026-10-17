export { YOUTUBE_BRIDGE } from './generated/pageBridge.js';

export function seekScript(videoId, seconds) {
  return `window.__lingoplay?.seek(${JSON.stringify(videoId)}, ${JSON.stringify(seconds)}); true;`;
}

export function playbackPositionScript(requestId, videoId, playback = false) {
  return `window.__lingoplay?.position(${JSON.stringify(requestId)}, ${JSON.stringify(videoId)}, ${playback === true}); true;`;
}

export function pauseScript(requestId, videoId, seconds) {
  return `window.__lingoplay?.pause(${JSON.stringify(requestId)}, ${JSON.stringify(videoId)}, ${JSON.stringify(seconds)}); true;`;
}

export function cancelPositionScript(requestId) {
  return `window.__lingoplay?.cancelPosition(${JSON.stringify(requestId)}); true;`;
}
