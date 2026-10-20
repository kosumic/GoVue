export const YOUTUBE_HOME = 'https://www.youtube.com/';

export function browserAddress(input: string): string {
  const value = input.trim();
  if (!value) throw new Error('Enter a website or a search.');
  const hasScheme = /^[a-z][a-z\d+.-]*:/i.test(value);
  if (hasScheme && !/^https?:\/\//i.test(value) && !/^[\w.-]+:\d+(?:\/|$)/.test(value)) {
    throw new Error('Only HTTP and HTTPS websites can be opened.');
  }
  const isAddress = hasScheme || /^(?:localhost|[\w-]+(?:\.[\w-]+)+|\[[\da-f:]+\])(?::\d+)?(?:[/?#]|$)/i.test(value);
  if (!isAddress || /\s/.test(value)) {
    return `https://www.google.com/search?q=${encodeURIComponent(value)}`;
  }
  const url = new URL(/^https?:\/\//i.test(value) ? value : `https://${value}`);
  if (!['https:', 'http:'].includes(url.protocol) || !url.hostname || url.username || url.password) {
    throw new Error('Enter a valid website address without embedded credentials.');
  }
  if (url.hostname === 'youtu.be') {
    const id = url.pathname.split('/')[1];
    if (/^[\w-]{11}$/.test(id)) {
      const video = new URL('https://www.youtube.com/watch');
      video.searchParams.set('v', id);
      const time = url.searchParams.get('t');
      if (time) video.searchParams.set('t', time);
      return video.href;
    }
  }
  return url.href;
}

export function isYouTubeUrl(input: string): boolean {
  try {
    const url = new URL(input);
    return url.protocol === 'https:' && ['youtube.com', 'www.youtube.com', 'm.youtube.com', 'music.youtube.com'].includes(url.hostname);
  } catch {
    return false;
  }
}

export function youtubeVideoId(input: string): string | null {
  try {
    const url = new URL(input);
    if (!isYouTubeUrl(input)) return null;
    const id = url.pathname === '/watch'
      ? url.searchParams.get('v')
      : /^\/(?:shorts|live|embed)\/([\w-]{11})(?:\/|$)/.exec(url.pathname)?.[1];
    return id && /^[\w-]{11}$/.test(id) ? id : null;
  } catch {
    return null;
  }
}
