import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const [input = '.cache/readme-preview/raw', ...requested] = process.argv.slice(2);
const names = ['browse', 'playback', 'transcript', 'search', 'word-card', 'languages', 'chat', 'providers', 'download', 'extensions'];
const selected = requested.length ? requested : names;
if (selected.some(name => !names.includes(name))) throw Error('Unknown README screenshot name.');
const source = resolve(root, input);
const output = join(root, 'assets/screenshots');
if (source === output) throw Error('Keep original PNG captures separate from the framed output.');
const width = 706;
const height = Math.round(width * (385 / 415) * (2622 / 1206) / (819 / 843));

function pngSize(png: Buffer) {
  if (png.subarray(0, 8).toString('hex') !== '89504e470d0a1a0a') throw Error('Expected an original PNG capture.');
  return { width: png.readUInt32BE(16), height: png.readUInt32BE(20) };
}

function run(command: string, args: string[]) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw Error(`${command} failed: ${result.stderr || result.stdout}`);
}

const frame = await readFile(join(root, 'assets/frames/iphone.svg'), 'utf8');
const cache = join(root, '.cache/readme-preview');
await mkdir(cache, { recursive: true });
const temporary = await mkdtemp(join(cache, 'mockups-'));
try {
  // Render everything before replacing any existing gallery images.
  for (const name of selected) {
    const png = await readFile(join(source, `${name}.png`));
    const size = pngSize(png);
    if (size.width !== 1206 || size.height !== 2622) {
      throw Error(`${name}.png must be an unframed 1206 × 2622 simulator capture.`);
    }
    const x = width * 15 / 415;
    const y = height * 12 / 843;
    const screenWidth = width * 385 / 415;
    const screenHeight = height * 819 / 843;
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
      <defs><clipPath id="screen"><rect x="${x}" y="${y}" width="${screenWidth}" height="${screenHeight}" rx="${width * 50 / 415}"/></clipPath></defs>
      <g clip-path="url(#screen)">
        <rect x="${x}" y="${y}" width="${screenWidth}" height="${screenHeight}" fill="#000"/>
        <image x="${x}" y="${y}" width="${screenWidth}" height="${screenHeight}" preserveAspectRatio="xMidYMid meet" href="data:image/png;base64,${png.toString('base64')}"/>
      </g>
      ${frame.replace('<svg ', '<svg x="0" y="0" preserveAspectRatio="none" ').replace('width="415"', `width="${width}"`).replace('height="843"', `height="${height}"`)}
    </svg>`;
    const rendered = join(temporary, `${name}.png`);
    await writeFile(join(temporary, `${name}.svg`), svg);
    run('resvg', [join(temporary, `${name}.svg`), rendered]);
    const renderedSize = pngSize(await readFile(rendered));
    if (renderedSize.width !== width || renderedSize.height !== height) throw Error(`Unexpected mockup size for ${name}.`);
    run('cwebp', ['-quiet', '-lossless', '-m', '6', '-mt', '-exact', rendered, '-o', join(temporary, `${name}.webp`)]);
  }
  await mkdir(output, { recursive: true });
  for (const name of selected) {
    await rename(join(temporary, `${name}.webp`), join(output, `${name}.webp`));
    console.log(`Framed ${name}.webp (${width} × ${height})`);
  }
} finally {
  await rm(temporary, { recursive: true, force: true });
}
