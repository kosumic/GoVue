import { execFileSync, type ExecFileSyncOptions } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, copyFileSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const moduleRoot = join(root, 'modules/govue-core');
const platform = process.argv[2] || 'host';
let gitCommit = '';
try {
  gitCommit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
} catch { /* Source archives may not have Git metadata. */ }
const run = (command: string, args: string[], extra: ExecFileSyncOptions = {}) => execFileSync(command, args, { cwd: root, stdio: 'inherit', env: { ...process.env, GOVUE_GIT_COMMIT: gitCommit, CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS || '2' }, ...extra });
const lib = process.platform === 'darwin' ? 'libgovue_core.dylib' : 'libgovue_core.so';

// Generate bindings from the host library; the host CLI is not a mobile input.
run('cargo', ['build', '--locked', '--release', '-p', 'govue-core', '-p', 'govue-bindgen', '--lib', '--bin', 'govue-bindgen']);
for (const language of ['kotlin', 'swift']) {
  const output = join(moduleRoot, language === 'kotlin' ? 'generated/kotlin' : 'ios/generated');
  mkdirSync(output, { recursive: true });
  run(join(root, 'target/release/govue-bindgen'), ['generate', '--library', join(root, 'target/release', lib), '--language', language, '--out-dir', output, '--no-format']);
  if (language === 'swift') {
    // The C ABI is private to this Expo module. Consumers of its Swift module
    // should not need the Rust FFI module in their own compiler search paths.
    const swift = join(output, 'GoVueRust.swift');
    const source = readFileSync(swift, 'utf8');
    if (!source.includes('\nimport GoVueRustFFI\n')) throw Error('Unexpected UniFFI Swift import; check the pinned generator.');
    writeFileSync(swift, source.replace('\nimport GoVueRustFFI\n', '\n@_implementationOnly import GoVueRustFFI\n'));
  }
}

if (platform === 'android') {
  const sdk = process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT;
  if (!sdk) throw Error('Use nix develop path:. for the Android SDK and Rust toolchain.');
  const target = 'aarch64-linux-android';
  run('cargo', ['build', '--locked', '--release', '-p', 'govue-core', '--lib', '--target', target]);
  const output = join(moduleRoot, 'android/src/main/jniLibs/arm64-v8a');
  mkdirSync(output, { recursive: true });
  copyFileSync(join(root, 'target', target, 'release/libgovue_core.so'), join(output, 'libgovue_core.so'));
} else if (platform === 'ios') {
  if (process.platform !== 'darwin') throw Error('The iOS core must be built on a Mac with Xcode.');
  // Nix's macOS SDK is suitable for host tools, but iOS targets need Xcode's
  // device/simulator SDKs rather than the SDK injected by the development shell.
  const discoveryEnv = { ...process.env };
  delete discoveryEnv.DEVELOPER_DIR;
  delete discoveryEnv.SDKROOT;
  const appleEnv = {
    ...process.env, GOVUE_GIT_COMMIT: gitCommit, CARGO_BUILD_JOBS: '2',
    PATH: '/usr/bin:/bin:' + process.env.PATH,
    DEVELOPER_DIR: execFileSync('/usr/bin/xcode-select', ['-p'], { encoding: 'utf8', env: discoveryEnv }).trim(),
    IPHONEOS_DEPLOYMENT_TARGET: '16.4',
  };
  for (const target of ['aarch64-apple-ios', 'aarch64-apple-ios-sim']) {
    const sdk = target.endsWith('-sim') ? 'iphonesimulator' : 'iphoneos';
    const SDKROOT = execFileSync('/usr/bin/xcrun', ['--sdk', sdk, '--show-sdk-path'], { encoding: 'utf8', env: appleEnv }).trim();
    run('cargo', ['build', '--locked', '--release', '-p', 'govue-core', '--lib', '--target', target], { env: { ...appleEnv, SDKROOT } });
  }
  const headers = join(moduleRoot, 'generated/headers');
  mkdirSync(headers, { recursive: true });
  for (const name of readdirSync(join(moduleRoot, 'ios/generated')).filter(name => name.endsWith('.h'))) {
    copyFileSync(join(moduleRoot, 'ios/generated', name), join(headers, name));
  }
  const framework = join(moduleRoot, 'ios/GoVueRust.xcframework');
  if (existsSync(framework)) run('rm', ['-rf', framework]);
  run('/usr/bin/xcodebuild', ['-create-xcframework',
    '-library', join(root, 'target/aarch64-apple-ios/release/libgovue_core.a'), '-headers', headers,
    '-library', join(root, 'target/aarch64-apple-ios-sim/release/libgovue_core.a'), '-headers', headers,
    '-output', framework], { env: appleEnv });
} else if (platform !== 'host') throw Error(`Unsupported core build platform: ${platform}`);
