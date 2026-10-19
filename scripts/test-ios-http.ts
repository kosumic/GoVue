import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

if (process.platform !== 'darwin') throw Error('The iOS HTTP checks require a Mac with Xcode.');
const directory = mkdtempSync(join(tmpdir(), 'govue-ios-http-'));
try {
  const binary = join(directory, 'ios-http-checks');
  execFileSync('xcrun', ['--sdk', 'macosx', 'swiftc', '-swift-version', '5',
    'modules/govue-core/ios/NativeHttpSession.swift',
    'modules/govue-core/ios/NativeMediaHttp.swift',
    'modules/govue-core/ios/BoundedHttp.swift', 'tests/iosHttp.swift', '-o', binary], { stdio: 'inherit' });
  execFileSync(binary, [], { stdio: 'inherit', timeout: 30_000 });
} finally { rmSync(directory, { recursive: true, force: true }); }
