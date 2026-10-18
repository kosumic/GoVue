import { readFileSync, writeFileSync } from 'node:fs';

// expo-translate-text 0.3.0 still loads ExpoModulesCorePlugin.gradle, removed in
// modern Expo SDKs. Preserve its native dependencies while using Expo's plugin.
const path = 'node_modules/expo-translate-text/android/build.gradle';
const source = readFileSync(path, 'utf8');
if (!source.includes("apply plugin: 'expo-module-gradle-plugin'")) {
  const start = source.indexOf('def expoModulesCorePlugin =');
  const end = source.indexOf('\nandroid {', start);
  if (start < 0 || end < 0 || !source.includes('applyKotlinExpoModulesCorePlugin()')) {
    throw Error('expo-translate-text changed; review the Android Expo plugin patch.');
  }
  writeFileSync(path, source.slice(0, start) + "apply plugin: 'expo-module-gradle-plugin'\n" + source.slice(end));
}
