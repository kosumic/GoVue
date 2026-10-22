import fs from 'node:fs';
import path from 'node:path';

// Original GoVue score: 45 seconds at 120 BPM. No samples or external music.
const rate = 48000;
const duration = 45;
const count = duration * rate;
const left = new Float32Array(count);
const right = new Float32Array(count);
const beat = 60 / 120;
let randomState = 41719;
const noise = () => { randomState = (Math.imul(randomState, 1664525) + 1013904223) >>> 0; return randomState / 2147483648 - 1; };
function add(start, length, sample, pan = 0) {
  const first = Math.floor(start * rate);
  const n = Math.min(Math.floor(length * rate), count - first);
  const l = Math.sqrt((1 - pan) / 2), r = Math.sqrt((1 + pan) / 2);
  for (let i = 0; i < n; i++) {
    const v = sample(i / rate, i);
    left[first + i] += v * l;
    right[first + i] += v * r;
  }
}
const hz = midi => 440 * 2 ** ((midi - 69) / 12);
for (let b = 0; b < 90; b++) {
  const t = b * beat;
  const bar = Math.floor(b / 4);
  const sparse = bar < 3 || bar >= 20;
  if (!(bar === 13 && b % 4 > 1) && t < 41) {
    add(t, .34, x => Math.sin(2 * Math.PI * (48 * x + 5 * (1 - Math.exp(-x * 28)))) * Math.exp(-x * 16) * .72);
  }
  if (b % 2 === 1 && !sparse) {
    add(t, .17, x => (noise() * .42 + Math.sin(2 * Math.PI * 185 * x) * .12) * Math.exp(-x * 30));
    for (let c = 0; c < 3; c++) add(t + c * .009, .08, x => noise() * Math.exp(-x * 54) * .15, c === 0 ? -.25 : .25);
  }
  if (bar > 1 && t < 40) for (let h = 0; h < 2; h++) {
    let previous = 0;
    add(t + beat * (.5 * h + .5), .055, x => { const v = noise(); const out = v - previous; previous = v; return out * Math.exp(-x * 85) * (h ? .075 : .12); }, h ? .48 : -.48);
  }
  const bass = [33, 33, 29, 36][Math.floor(bar / 2) % 4];
  if (t < 41) for (const off of sparse ? [.5] : [0, .5, .75]) {
    const f = hz(bass + (off === .75 ? 12 : 0));
    add(t + beat * off, beat * .32, x => {
      const env = Math.min(1, x * 140) * Math.exp(-x * 14);
      return (Math.sin(2 * Math.PI * f * x) + .2 * Math.sin(4 * Math.PI * f * x)) * env * .23;
    });
  }
}
// Bright plucked motif, with delay taps and a calmer last eight beats.
const melody = [69, 76, 81, 76, 72, 79, 84, 79, 65, 72, 77, 72, 67, 74, 79, 74];
for (let n = 0; n < 144; n++) {
  const t = 6 + n * beat / 2;
  if (t >= 40.5) break;
  const f = hz(melody[n % melody.length]);
  const voice = x => {
    const env = Math.min(1, x * 400) * Math.exp(-x * 8);
    return (Math.sin(2 * Math.PI * f * x) + .25 * Math.sin(4 * Math.PI * f * x) + .08 * Math.sin(6 * Math.PI * f * x)) * env * .105;
  };
  add(t, .7, voice, n % 2 ? -.25 : .25);
  add(t + beat * .75, .7, x => voice(x) * .32, n % 2 ? .5 : -.5);
  add(t + beat * 1.5, .7, x => voice(x) * .13, n % 2 ? -.7 : .7);
}
// Scene transitions have their own timed whoosh and low impact.
for (const t of [7.1, 12, 20, 27, 29.7, 33, 41]) {
  add(t - .58, .8, x => {
    const env = x < .58 ? (x / .58) ** 2 : Math.exp(-(x - .58) * 22);
    return (noise() * .13 + Math.sin(2 * Math.PI * (130 * x + 520 * x * x)) * .025) * env;
  }, -.15);
  add(t, .55, x => Math.sin(2 * Math.PI * (39 * x + 2.5 * (1 - Math.exp(-x * 20)))) * Math.exp(-x * 10) * .4);
}
// Quiet selection ticks connect the two blue bands to the opening action.
for (const t of [1.1, 1.3]) add(t, .055, x => Math.sin(2 * Math.PI * 1100 * x) * Math.exp(-x * 95) * .11);
// Final A minor add9 chord and a short sparkling resolution.
for (const [i, midi] of [57, 64, 69, 71, 76, 81].entries()) {
  const f = hz(midi);
  add(41 + i * .055, 4 - i * .055, x => Math.sin(2 * Math.PI * f * x) * Math.min(1, x * 8) * Math.exp(-x * .75) * .105, (i - 2.5) / 5);
}
let peak = 0;
for (let i = 0; i < count; i++) peak = Math.max(peak, Math.abs(left[i]), Math.abs(right[i]));
const gain = .86 / peak;
const pcm = Buffer.alloc(count * 4);
for (let i = 0; i < count; i++) {
  const edge = Math.min(1, i / (rate * .015), (count - 1 - i) / (rate * .2));
  pcm.writeInt16LE(Math.round(Math.max(-1, Math.min(1, left[i] * gain * edge)) * 32767), i * 4);
  pcm.writeInt16LE(Math.round(Math.max(-1, Math.min(1, right[i] * gain * edge)) * 32767), i * 4 + 2);
}
const header = Buffer.alloc(44);
header.write('RIFF', 0); header.writeUInt32LE(36 + pcm.length, 4); header.write('WAVEfmt ', 8);
header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); header.writeUInt16LE(2, 22);
header.writeUInt32LE(rate, 24); header.writeUInt32LE(rate * 4, 28); header.writeUInt16LE(4, 32);
header.writeUInt16LE(16, 34); header.write('data', 36); header.writeUInt32LE(pcm.length, 40);
const output = path.resolve(import.meta.dirname, '../assets/audio/govue-score.wav');
fs.writeFileSync(output, Buffer.concat([header, pcm]));
console.log(`Original stereo score: ${duration}s, 120 BPM → ${output}`);
