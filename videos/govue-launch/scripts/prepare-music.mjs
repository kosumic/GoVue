import path from 'node:path';
import { execFileSync } from 'node:child_process';
const root = path.resolve(import.meta.dirname, '..');
execFileSync(process.env.HYPERFRAMES_FFMPEG_PATH || 'ffmpeg', [
  '-hide_banner', '-loglevel', 'error', '-y', '-i', path.join(root, 'assets/audio/tech-house-vibes.source.mp3'),
  '-t', '45', '-vn', '-af', 'loudnorm=I=-15:TP=-1.5:LRA=7,afade=t=in:d=0.15,afade=t=out:st=43.8:d=1.2',
  '-ar', '48000', '-ac', '2', path.join(root, 'assets/audio/govue-v3-music.wav'),
]);
console.log('Conformed licensed Mixkit track: 45 seconds, stereo, with fades.');
