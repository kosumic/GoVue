import { readFileSync, writeFileSync } from 'node:fs';

// Expo Video 57 captures currentActivity when its module starts. On a cold
// start that can be null; manual PiP then silently returns without entering.
const path = 'node_modules/expo-video/android/src/main/java/expo/modules/video/managers/PictureInPictureManager.kt';
const before = 'private val mainActivity = WeakReference(appContext.currentActivity)';
const after = `private val initialActivity = WeakReference(appContext.currentActivity)
  private val mainActivity
    get() = WeakReference(this.appContext.get()?.currentActivity ?: initialActivity.get())`;
const source = readFileSync(path, 'utf8');
if (source.includes(before)) writeFileSync(path, source.replace(before, after));
else if (!source.includes(after)) throw Error('Expo Video PiP manager changed; review the cold-start activity patch.');
