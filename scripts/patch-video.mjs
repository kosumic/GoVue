import { readFileSync, writeFileSync } from 'node:fs';

// Expo Video 57 captures currentActivity when its module starts. On a cold
// start that can be null; manual PiP then silently returns without entering.
// Keep the last live host weakly: currentActivity can become null again while
// backgrounding, when PiP still needs to update the activity's native layout.
const path = 'node_modules/expo-video/android/src/main/java/expo/modules/video/managers/PictureInPictureManager.kt';
const before = 'private val mainActivity = WeakReference(appContext.currentActivity)';
const previousActivityPatch = `private val initialActivity = WeakReference(appContext.currentActivity)
  private val mainActivity
    get() = WeakReference(this.appContext.get()?.currentActivity ?: initialActivity.get())`;
const after = `private var lastActivity = WeakReference<android.app.Activity>(appContext.currentActivity)
  private val mainActivity
    get() = this.appContext.get()?.currentActivity?.let { activity ->
      WeakReference(activity).also { lastActivity = it }
    } ?: lastActivity`;
let source = readFileSync(path, 'utf8');
function patch(before, after, name) {
  if (source.includes(after)) return;
  if (!source.includes(before)) throw Error(`Expo Video PiP manager changed; review the ${name} patch.`);
  source = source.replace(before, after);
}
if (source.includes(previousActivityPatch)) source = source.replace(previousActivityPatch, after);
else patch(before, after, 'cold-start activity');

// RN Modals own separate Android Dialog windows. Hiding the React root does
// not hide those windows, and a JS visibility update can wait until foreground.
// Hide them synchronously with the native PiP layout, preserving sheet state.
patch('import android.app.PictureInPictureParams', 'import android.app.Dialog\nimport android.app.PictureInPictureParams', 'dialog import');
patch('import expo.modules.kotlin.AppContext', 'import com.facebook.react.views.modal.ReactModalHostView\nimport expo.modules.kotlin.AppContext', 'modal import');
patch('  private var isReleased = false', '  private val hiddenPiPDialogs = mutableSetOf<Dialog>()\n  private var isReleased = false', 'dialog state');
patch('  private fun layoutForPiPEnter(videoView: VideoView) {', `  private fun hideDialogWindows(view: View) {
    if (view is ReactModalHostView) {
      view.dialog?.let { dialog ->
        if (dialog.isShowing && dialog.window?.decorView?.visibility == View.VISIBLE) {
          hiddenPiPDialogs.add(dialog)
          dialog.hide()
        }
      }
    }
    if (view is ViewGroup) {
      for (i in 0 until view.childCount) {
        view.getChildAt(i)?.let { hideDialogWindows(it) }
      }
    }
  }

  private fun layoutForPiPEnter(videoView: VideoView) {`, 'dialog visibility');
patch('    (playerView.parent as? ViewGroup)?.removeView(playerView)', '    hideDialogWindows(rootView)\n    (playerView.parent as? ViewGroup)?.removeView(playerView)', 'dialog hide');
const previousDialogRestore = `    rootViewChildrenOriginalVisibility.clear()
    if (!mainActivity.isFinishing && !mainActivity.isDestroyed) {
      hiddenPiPDialogs.forEach { if (it.isShowing) it.show() }
    }
    hiddenPiPDialogs.clear()`;
const dialogRestore = `    rootViewChildrenOriginalVisibility.clear()
    if (!mainActivity.isFinishing && !mainActivity.isDestroyed) {
      hiddenPiPDialogs.forEach { if (it.window?.decorView?.isAttachedToWindow == true) it.show() }
    }
    hiddenPiPDialogs.clear()`;
// Dialog.isShowing is false after hide(); an attached window is still live.
if (source.includes(previousDialogRestore)) source = source.replace(previousDialogRestore, dialogRestore);
else patch('    rootViewChildrenOriginalVisibility.clear()', dialogRestore, 'dialog restore');

writeFileSync(path, source);
