import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { parseArgs } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';

// Run on mini. Capture real simulator events; preserve earlier takes.
const { values } = parseArgs({ options: {
  name: { type: 'string' }, duration: { type: 'string' }, plan: { type: 'string' },
  device: { type: 'string', default: 'FB908D0D-3294-4745-BFC5-ACAF397EC3C8' },
  axe: { type: 'string', default: '/Users/kosumi/repos/whip-terminal-bridge-20261020/build/launch-refresh/axe/axe' },
  output: { type: 'string', default: '/Users/kosumi/repos/GoVue/.cache/launch-v3/takes' },
} });
if (!values.name || !values.duration || !values.plan) throw new Error('Use --name TAKE --duration SECONDS --plan ACTIONS.json');
const actions = JSON.parse(fs.readFileSync(values.plan, 'utf8'));
fs.mkdirSync(values.output, { recursive: true });
const base = path.join(values.output, values.name);
if (fs.existsSync(`${base}.mov`)) throw new Error(`Keep the earlier take: ${base}.mov`);
process.env.AXE_HID_STABILIZATION_MS = '25';
const run = (program, args) => new Promise((resolve, reject) => {
  const p = spawn(program, args, { stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '';
  p.stdout.on('data', b => { output += b; }); p.stderr.on('data', b => { output += b; });
  p.on('error', reject); p.on('exit', code => code === 0 ? resolve(output) : reject(new Error(output)));
});
let recorderLog = '';
const recorder = spawn('xcrun', ['simctl', 'io', values.device, 'recordVideo', '--codec=h264', `${base}.mov`]);
recorder.stdout.on('data', b => { recorderLog += b; });
recorder.stderr.on('data', b => { recorderLog += b; });
const stopped = new Promise(resolve => recorder.on('exit', resolve));
const actual = [];
try {
  for (let i = 0; !recorderLog.includes('Recording started'); i++) {
    if (i > 200 || recorder.exitCode !== null) throw new Error(`Recorder did not start: ${recorderLog}`);
    await delay(100);
  }
  const start = process.hrtime.bigint();
  const elapsed = () => Number(process.hrtime.bigint() - start) / 1e9;
  console.log(`Recording ${values.name}`);
  for (const action of actions) {
    await delay(Math.max(0, (action.at - elapsed()) * 1000));
    const stamp = { ...action, actualAt: elapsed() };
    await run(values.axe, [...action.command, '--udid', values.device]);
    stamp.finishedAt = elapsed(); actual.push(stamp);
    console.log(`${stamp.actualAt.toFixed(2)}s ${action.command[0]}`);
  }
  await delay(Math.max(0, (Number(values.duration) - elapsed()) * 1000));
} finally {
  recorder.kill('SIGINT'); await stopped;
  fs.writeFileSync(`${base}.log`, recorderLog);
  fs.writeFileSync(`${base}.actions.json`, JSON.stringify(actual, null, 2) + '\n');
}
if (recorder.exitCode !== 0) throw new Error(recorderLog);
console.log(`Saved ${base}.mov`);
