import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';

const root = path.resolve(import.meta.dirname, '..');
const ffmpeg = process.env.HYPERFRAMES_FFMPEG_PATH || 'ffmpeg';
const ffprobe = process.env.HYPERFRAMES_FFPROBE_PATH || 'ffprobe';
const edits = JSON.parse(fs.readFileSync(path.join(root, 'capture/edits.json')));
const clips = [];
fs.mkdirSync(path.join(root, 'assets/footage'), { recursive: true });
for (const e of edits) {
  const source = path.resolve(root, e.raw), temp = path.join(root, `assets/footage/${e.name}.preparing.mp4`);
  const sourceDuration = e.segments?.reduce((sum, s) => sum + s.duration, 0) || e.sourceDuration || e.duration;
  const inputs = e.segments ? e.segments.map(s => path.resolve(root, s.raw || e.raw)) : [source];
  const tail = `setpts=(PTS-STARTPTS)*${e.duration / sourceDuration},fps=30,tpad=stop_mode=clone:stop_duration=0.1,trim=duration=${e.duration}`;
  // Editorial jump cuts retain actual screen pixels; omit system dialogs and waits.
  const filter = e.segments ? ['-filter_complex',
    e.segments.map((s, i) => `[${i}:v]fps=30,trim=start=${s.in}:duration=${s.duration},setpts=PTS-STARTPTS[part${i}]`).join(';') +
      ';' + e.segments.map((_, i) => `[part${i}]`).join('') + `concat=n=${e.segments.length}:v=1:a=0,${tail}[out]`, '-map', '[out]'] :
    // Pad before resampling so a final VFR frame shorter than 1/30s is retained.
    ['-vf', `${e.holdLastFrame ? `tpad=stop_mode=clone:stop_duration=${e.holdLastFrame},` : ''}fps=30,trim=start=${e.in}:duration=${sourceDuration},${tail}`];
  execFileSync(ffmpeg, ['-hide_banner', '-loglevel', 'error', '-y', ...inputs.flatMap(input => ['-i', input]), '-an', ...filter,
    '-c:v', 'libx264', '-crf', '18', '-preset', 'fast', '-pix_fmt', 'yuv420p', '-g', '10', '-movflags', '+faststart', temp]);
  const probe = JSON.parse(execFileSync(ffprobe, ['-v', 'error', '-count_frames', '-show_entries', 'stream=width,height,nb_read_frames:format=duration', '-of', 'json', temp], { encoding: 'utf8' }));
  if (Number(probe.streams[0].nb_read_frames) !== e.duration * 30) throw new Error(`${e.name}: incomplete live footage`);
  const hash = crypto.createHash('sha256').update(fs.readFileSync(temp)).digest('hex');
  const src = `assets/footage/${e.name}.${hash.slice(0, 12)}.mp4`; fs.renameSync(temp, path.join(root, src));
  clips.push({ name: e.name, src, start: e.start, duration: e.duration, sha256: hash, raw: path.basename(source), sourceIn: e.in,
    width: probe.streams[0].width, height: probe.streams[0].height, fps: 30, sourceDuration, speed: sourceDuration / e.duration,
    ...(e.segments ? { segments: e.segments } : {}),
    ...(e.holdLastFrame ? { holdLastFrame: e.holdLastFrame } : {}),
    ...(e.note ? { editNote: e.note } : {}) });
  console.log(`${e.name}: ${e.duration}s real recording → ${src}`);
}
fs.writeFileSync(path.join(root, 'capture/footage.json'), JSON.stringify(clips, null, 2) + '\n');
