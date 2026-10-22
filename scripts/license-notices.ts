import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

interface Notice {
  id: string;
  title: string;
  license: string;
  category: 'npm' | 'cargo';
  sourceUrl: string;
  attribution: string;
  text: string;
}

interface Package {
  name: string;
  version: string;
  license?: string;
  repository?: string | { url?: string };
}

interface CargoReport {
  licenses: {
    id: string;
    text: string;
    used_by: { crate: Package & { repository?: string | null } }[];
  }[];
}

interface Bundle {
  cargoInputs: string;
  notices: Notice[];
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const bundlePath = resolve(root, 'notices/dependencies.json');
const write = process.argv.includes('--write');
const read = (path: string) => readFile(resolve(root, path), 'utf8');
const sort = (a: Notice, b: Notice) => a.title.localeCompare(b.title, 'en') || a.id.localeCompare(b.id, 'en');

function sourceUrl(repository: Package['repository'], fallback: string): string {
  const raw = typeof repository === 'string' ? repository : repository?.url;
  const url = raw?.replace(/^git\+/, '').replace(/^git:\/\//, 'https://')
    .replace(/^git@github\.com:/, 'https://github.com/').replace(/\.git$/, '');
  return url?.startsWith('https://') ? url : fallback;
}

async function npmNotices(): Promise<Notice[]> {
  const manifest = JSON.parse(await read('package.json')) as { dependencies: Record<string, string> };
  const lock = JSON.parse(await read('package-lock.json')) as { packages: Record<string, Package> };
  return Promise.all(Object.keys(manifest.dependencies).sort().map(async name => {
    const directory = `node_modules/${name}`;
    const pkg = JSON.parse(await read(`${directory}/package.json`)) as Package;
    if (pkg.version !== lock.packages[directory]?.version) {
      throw Error(`${name} differs from package-lock.json; run npm ci first.`);
    }
    if (!pkg.license || /^(unknown|unlicensed|none)$/i.test(pkg.license)) {
      throw Error(`Missing license metadata for ${name}.`);
    }
    const files = (await readdir(resolve(root, directory), { withFileTypes: true }))
      .filter(file => file.isFile() && /^(licen[cs]e|copying|notice)([._-].*)?$/i.test(file.name))
      .map(file => file.name).sort();
    if (!files.length) throw Error(`No bundled license text for ${name}.`);
    const text = (await Promise.all(files.map(file => read(`${directory}/${file}`)))).join('\n\n');
    if (text.trim().length < 100) throw Error(`Incomplete license text for ${name}.`);
    return {
      id: `npm:${name}@${pkg.version}`,
      title: name,
      license: name === 'lucide-react-native' ? 'ISC AND MIT' : pkg.license,
      category: 'npm',
      sourceUrl: sourceUrl(pkg.repository, `https://www.npmjs.com/package/${name}`),
      attribution: `Direct npm dependency · version ${pkg.version}`,
      text,
    };
  }));
}

function cargoNotices(): Notice[] {
  const report = JSON.parse(execFileSync('cargo-about', [
    'generate', '--locked', '--fail', '--manifest-path', 'core/Cargo.toml',
    '--config', 'notices/cargo-about.toml', '--format', 'json',
    '--target', 'aarch64-linux-android', '--target', 'aarch64-apple-ios',
  ], { cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, timeout: 120_000 })) as CargoReport;
  const crates = new Map<string, Notice>();
  for (const license of report.licenses) {
    for (const { crate: pkg } of license.used_by) {
      // cargo-about includes the selected workspace package in its report.
      if (pkg.name === 'govue-core') continue;
      const id = `cargo:${pkg.name}@${pkg.version}`;
      const existing = crates.get(id);
      const text = `${license.id}\n\n${license.text}`;
      if (existing) {
        existing.license += ` AND ${license.id}`;
        existing.text += `\n\n${text}`;
      } else {
        crates.set(id, {
          id, title: pkg.name, license: license.id, category: 'cargo',
          sourceUrl: sourceUrl(pkg.repository ?? undefined, `https://crates.io/crates/${pkg.name}`),
          attribution: `Direct Rust dependency · version ${pkg.version}`,
          text,
        });
      }
    }
  }
  if (!crates.size) throw Error('cargo-about returned no dependency notices.');
  return [...crates.values()].sort(sort);
}

// Check Rust inputs without requiring a second Rust toolchain in the JS CI job.
// Regeneration uses cargo-about to resolve the actual Android/iOS license texts.
const cargoHash = createHash('sha256');
for (const path of ['Cargo.toml', 'core/Cargo.toml', 'Cargo.lock', 'notices/cargo-about.toml']) {
  cargoHash.update(path).update('\0').update(await read(path)).update('\0');
}
const cargoInputs = cargoHash.digest('hex');
const npm = (await npmNotices()).sort(sort);
if (write) {
  const bundle: Bundle = { cargoInputs, notices: [...npm, ...cargoNotices()] };
  await writeFile(bundlePath, `${JSON.stringify(bundle, null, 2)}\n`);
  console.log(`Bundled ${bundle.notices.length} npm and Rust dependency notices.`);
} else {
  const bundle = JSON.parse(await readFile(bundlePath, 'utf8')) as Bundle;
  if (bundle.cargoInputs !== cargoInputs ||
    JSON.stringify(bundle.notices.filter(notice => notice.category === 'npm')) !== JSON.stringify(npm)) {
    throw Error('Bundled license notices are stale. Run npm run licenses:generate inside nix develop path:.');
  }
  for (const notice of bundle.notices) {
    if (!notice.text || notice.text.trim().length < 100 || !notice.sourceUrl.startsWith('https://')) {
      throw Error(`Incomplete bundled notice: ${notice.id}`);
    }
  }
  console.log(`Checked ${bundle.notices.length} bundled dependency notices.`);
}
