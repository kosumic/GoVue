const assert = require('node:assert/strict');
const path = require('node:path');
const { getConfig } = require('expo/config');
const { AndroidConfig, IOSConfig } = require('expo/config-plugins');
const { assertVersion, version } = require('./app-version.cjs');

const root = path.resolve(__dirname, '..');
const { exp } = getConfig(root);
const staticConfig = require('../app.json').expo;

for (const config of [staticConfig, staticConfig.ios, staticConfig.android]) {
  assert(!Object.hasOwn(config || {}, 'version'), 'Keep app versions in package.json, not app.json');
}
assertVersion(exp.version, 'Expo version');
assertVersion(AndroidConfig.Version.getVersionName(exp), 'Expo Android version');
assertVersion(IOSConfig.Version.getVersion(exp), 'Expo iOS version');
assert(!Object.hasOwn(exp.ios?.infoPlist || {}, 'CFBundleShortVersionString'),
  'Remove the Expo Info.plist marketing version override');

console.log(`App version configuration: ${version} (package.json, Expo, Android, and iOS)`);
