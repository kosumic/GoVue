import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const options = new Map();
for (let index = 0; index < args.length; index += 2) {
  if (!['--duration', '--output', '--serial'].includes(args[index]) || !args[index + 1]) {
    throw Error('Usage: node scripts/profile-transcript.mjs [--duration seconds] [--output file.pftrace] [--serial adb-device]');
  }
  options.set(args[index], args[index + 1]);
}
const duration = Number(options.get('--duration') ?? 60);
if (!Number.isInteger(duration) || duration < 10 || duration > 300) throw Error('Duration must be 10–300 seconds.');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const output = resolve(options.get('--output') ?? `.cache/transcript-${stamp}.pftrace`);
const remote = `/data/misc/perfetto-traces/lingoplay-transcript-${stamp}.pftrace`;
const adbArgs = options.has('--serial') ? ['-s', options.get('--serial')] : [];
const config = readFileSync(fileURLToPath(new URL('./transcript.perfetto.pbtxt', import.meta.url)), 'utf8')
  .replace('duration_ms: 60000', `duration_ms: ${duration * 1000}`);
mkdirSync(dirname(output), { recursive: true });
console.log(`Recording ${duration}s with Perfetto. Open a video and load or refresh its transcript on the phone.`);
const recorder = spawn('adb', [...adbArgs, 'shell', 'perfetto', '--txt', '-c', '-', '-o', remote], {
  stdio: ['pipe', 'inherit', 'inherit'],
});
recorder.stdin.end(config);
await new Promise((accept, reject) => {
  recorder.on('error', reject);
  recorder.on('exit', code => code === 0 ? accept() : reject(Error(`Perfetto exited with ${code}.`)));
});
execFileSync('adb', [...adbArgs, 'pull', remote, output], { stdio: 'inherit' });
execFileSync('adb', [...adbArgs, 'shell', 'rm', remote]);
console.log(`Saved ${output}\nOpen it in https://ui.perfetto.dev and search for "LingoPlay transcript:".`);
