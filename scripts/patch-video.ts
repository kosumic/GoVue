import { readFileSync, writeFileSync } from 'node:fs';

// Expo Video 57 captures currentActivity when its module starts. On a cold
// start that can be null; manual PiP then silently returns without entering.
// Keep the last live host weakly: currentActivity can become null again while
// backgrounding, when PiP still needs to update the activity's native layout.
const path =
  'node_modules/expo-video/android/src/main/java/expo/modules/video/managers/PictureInPictureManager.kt';
const before =
  'private val mainActivity = WeakReference(appContext.currentActivity)';
const previousActivityPatch = `private val initialActivity = WeakReference(appContext.currentActivity)
  private val mainActivity
    get() = WeakReference(this.appContext.get()?.currentActivity ?: initialActivity.get())`;
const after = `private var lastActivity = WeakReference<android.app.Activity>(appContext.currentActivity)
  private val mainActivity
    get() = this.appContext.get()?.currentActivity?.let { activity ->
      WeakReference(activity).also { lastActivity = it }
    } ?: lastActivity`;
let source = readFileSync(path, 'utf8');
function patch(before: string, after: string, name: string) {
  if (source.includes(after)) return;
  if (!source.includes(before))
    throw Error(`Expo Video PiP manager changed; review the ${name} patch.`);
  source = source.replace(before, after);
}
if (source.includes(previousActivityPatch))
  source = source.replace(previousActivityPatch, after);
else patch(before, after, 'cold-start activity');

// RN Modals own separate Android Dialog windows. Hiding the React root does
// not hide those windows, and a JS visibility update can wait until foreground.
// Hide them synchronously with the native PiP layout, preserving sheet state.
patch(
  'import android.app.PictureInPictureParams',
  'import android.app.Dialog\nimport android.app.PictureInPictureParams',
  'dialog import',
);
patch(
  'import expo.modules.kotlin.AppContext',
  'import com.facebook.react.views.modal.ReactModalHostView\nimport expo.modules.kotlin.AppContext',
  'modal import',
);
patch(
  '  private var isReleased = false',
  '  private val hiddenPiPDialogs = mutableSetOf<Dialog>()\n  private var isReleased = false',
  'dialog state',
);
patch(
  '  private fun layoutForPiPEnter(videoView: VideoView) {',
  `  private fun hideDialogWindows(view: View) {
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

  private fun layoutForPiPEnter(videoView: VideoView) {`,
  'dialog visibility',
);
patch(
  '    (playerView.parent as? ViewGroup)?.removeView(playerView)',
  '    hideDialogWindows(rootView)\n    (playerView.parent as? ViewGroup)?.removeView(playerView)',
  'dialog hide',
);
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
if (source.includes(previousDialogRestore))
  source = source.replace(previousDialogRestore, dialogRestore);
else
  patch(
    '    rootViewChildrenOriginalVisibility.clear()',
    dialogRestore,
    'dialog restore',
  );

writeFileSync(path, source);

// Android can reject a service start after a foreground/background race. Keep
// the player alive; the shared UI retries its service configuration on resume.
const servicePath =
  'node_modules/expo-video/android/src/main/java/expo/modules/video/playbackService/ExpoVideoPlaybackService.kt';
let serviceSource = readFileSync(servicePath, 'utf8');
const serviceBefore = '        startService(intent)';
const serviceAfter = `        try {
          startService(intent)
        } catch (_: IllegalStateException) {
          return false
        }`;
if (!serviceSource.includes(serviceAfter)) {
  if (!serviceSource.includes(serviceBefore))
    throw Error(
      'Expo Video service changed; review the background-start patch.',
    );
  serviceSource = serviceSource.replace(serviceBefore, serviceAfter);
  writeFileSync(servicePath, serviceSource);
}

// Expo exposes video tracks read-only. Select a live HLS rendition using the
// same Media3 player so its audio, live position and mounted surface survive.
// Correlate the operation on the main queue to the exact current source.
const modulePath =
  'node_modules/expo-video/android/src/main/java/expo/modules/video/VideoModule.kt';
let moduleSource = readFileSync(modulePath, 'utf8');
const trackAnchor = '      Property("availableVideoTracks")';
const videoEnabled = `      AsyncFunction("lingoplaySetVideoEnabled") { ref: VideoPlayer, expectedUri: String, enabled: Boolean ->
        if (ref.player.currentMediaItem?.localConfiguration?.uri?.toString() != expectedUri) false
        else {
          ref.player.trackSelectionParameters = ref.player.trackSelectionParameters.buildUpon()
            .setTrackTypeDisabled(androidx.media3.common.C.TRACK_TYPE_VIDEO, !enabled).build()
          true
        }
      }.runOnQueue(Queues.MAIN)

${trackAnchor}`;
if (!moduleSource.includes(videoEnabled.slice(0, -trackAnchor.length))) {
  if (moduleSource.includes('AsyncFunction("lingoplaySetVideoEnabled")'))
    throw Error(
      'Expo Video background renderer patch differs; review the adapter.',
    );
  if (!moduleSource.includes(trackAnchor))
    throw Error('Expo Video track property changed.');
  moduleSource = moduleSource.replace(trackAnchor, videoEnabled);
}
const selectTrack = `      AsyncFunction("lingoplaySelectVideoTrack") { ref: VideoPlayer, expectedUri: String, trackId: String? ->
        if (ref.player.currentMediaItem?.localConfiguration?.uri?.toString() != expectedUri) false
        else {
          val parameters = ref.player.trackSelectionParameters.buildUpon()
            .clearOverridesOfType(androidx.media3.common.C.TRACK_TYPE_VIDEO)
          if (trackId == null) {
            ref.player.trackSelectionParameters = parameters.build()
            true
          } else {
            var selected: androidx.media3.common.TrackSelectionOverride? = null
            for (group in ref.player.currentTracks.groups) {
              for (index in 0 until group.length) {
                val format = group.getTrackFormat(index)
                if (format.id == trackId && group.isTrackSupported(index) &&
                  androidx.media3.common.MimeTypes.isVideo(format.sampleMimeType)) {
                  selected = androidx.media3.common.TrackSelectionOverride(group.mediaTrackGroup, index)
                  break
                }
              }
              if (selected != null) break
            }
            if (selected == null) false
            else {
              ref.player.trackSelectionParameters = parameters.setOverrideForType(selected).build()
              true
            }
          }
        }
      }.runOnQueue(Queues.MAIN)

${trackAnchor}`;
// Other adapters can be inserted before the same property anchor. Check the
// complete method body independently so applying patches twice stays safe.
if (!moduleSource.includes(selectTrack.slice(0, -trackAnchor.length))) {
  if (moduleSource.includes('AsyncFunction("lingoplaySelectVideoTrack")'))
    throw Error(
      'Expo Video live selection patch differs; review the existing adapter.',
    );
  if (!moduleSource.includes(trackAnchor))
    throw Error('Expo Video tracks changed; review live rendition selection.');
  moduleSource = moduleSource.replace(trackAnchor, selectTrack);
}
const replaceAnchor = '      ref.uncommittedSource = videoSource';
const resetVideoOverride = `      ref.player.trackSelectionParameters = ref.player.trackSelectionParameters.buildUpon()
        .clearOverridesOfType(androidx.media3.common.C.TRACK_TYPE_VIDEO).build()
${replaceAnchor}`;
if (!moduleSource.includes(resetVideoOverride)) {
  if (!moduleSource.includes(replaceAnchor))
    throw Error(
      'Expo Video source replacement changed; review live selection cleanup.',
    );
  moduleSource = moduleSource.replace(replaceAnchor, resetVideoOverride);
}
writeFileSync(modulePath, moduleSource);

// Read renderer counters from the existing player. Only bounded metadata and
// numeric counters cross this adapter; source addresses and error text do not.
const statsAnchor = '      Property("availableVideoTracks")';
const statsMethod = `      AsyncFunction("lingoplayPlaybackStats") { ref: VideoPlayer, expectedUri: String ->
        if (ref.player.currentMediaItem?.localConfiguration?.uri?.toString() != expectedUri) null
        else {
          val player = ref.player
          val counters = player.videoDecoderCounters
          counters?.ensureUpdated()
          val video = player.videoFormat
          val audio = player.audioFormat
          val display = ref.context.getSystemService(android.hardware.display.DisplayManager::class.java)
            ?.getDisplay(android.view.Display.DEFAULT_DISPLAY)
          mapOf(
            "epoch" to ref.lingoplaySourceAt,
            "sampledAt" to android.os.SystemClock.elapsedRealtime(),
            "nominalFps" to video?.frameRate?.takeIf { it.isFinite() && it > 0 },
            "refreshRate" to display?.refreshRate?.takeIf { it.isFinite() && it > 0 },
            "speed" to player.playbackParameters.speed,
            "videoCodec" to (video?.codecs ?: video?.sampleMimeType),
            "audioCodec" to (audio?.codecs ?: audio?.sampleMimeType),
            "videoDecoder" to ref.lingoplayVideoDecoder,
            "audioDecoder" to ref.lingoplayAudioDecoder,
            "width" to video?.width, "height" to video?.height,
            "videoBitrate" to video?.bitrate, "audioBitrate" to audio?.bitrate,
            "audioSampleRate" to audio?.sampleRate, "audioChannels" to audio?.channelCount,
            "renderedFrames" to (counters?.renderedOutputBufferCount ?: 0),
            "droppedFrames" to (counters?.droppedBufferCount ?: 0),
            "firstFrameMs" to ref.lingoplayFirstFrameMs,
            "visible" to (ref.currentVideoView?.isShown == true),
            "state" to when {
              player.playerError != null -> "error"
              player.playbackState == androidx.media3.common.Player.STATE_BUFFERING -> "buffering"
              player.isPlaying -> "playing"
              player.playbackState == androidx.media3.common.Player.STATE_ENDED -> "ended"
              player.playbackState == androidx.media3.common.Player.STATE_IDLE -> "idle"
              else -> "paused"
            },
            "errorCode" to player.playerError?.errorCodeName
          )
        }
      }.runOnQueue(Queues.MAIN)

${statsAnchor}`;
if (!moduleSource.includes(statsMethod.slice(0, -statsAnchor.length))) {
  if (moduleSource.includes('AsyncFunction("lingoplayPlaybackStats")'))
    throw Error('Expo Video diagnostics patch differs; review the adapter.');
  if (!moduleSource.includes(statsAnchor))
    throw Error('Expo Video tracks changed; review playback diagnostics.');
  moduleSource = moduleSource.replace(statsAnchor, statsMethod);
}
writeFileSync(modulePath, moduleSource);

const playerPath =
  'node_modules/expo-video/android/src/main/java/expo/modules/video/player/VideoPlayer.kt';
let playerSource = readFileSync(playerPath, 'utf8');
function patchPlayer(before: string, after: string) {
  if (playerSource.includes(after)) return;
  if (!playerSource.includes(before))
    throw Error('Expo Video player changed; review playback diagnostics.');
  playerSource = playerSource.replace(before, after);
}
patchPlayer(
  '  private val analyticsListener = object : AnalyticsListener {',
  `  var lingoplaySourceAt: Long = 0L
  var lingoplayFirstFrameMs: Long? = null
  var lingoplayVideoDecoder: String? = null
  var lingoplayAudioDecoder: String? = null

  private fun lingoplayCurrentEvent(event: AnalyticsListener.EventTime): Boolean =
    !event.timeline.isEmpty && event.windowIndex in 0 until event.timeline.windowCount &&
      event.timeline.getWindow(event.windowIndex, Timeline.Window()).mediaItem.localConfiguration?.uri ==
        player.currentMediaItem?.localConfiguration?.uri

  private val analyticsListener = object : AnalyticsListener {
    override fun onVideoDecoderInitialized(eventTime: AnalyticsListener.EventTime, decoderName: String, initializedTimestampMs: Long, initializationDurationMs: Long) {
      if (lingoplayCurrentEvent(eventTime)) lingoplayVideoDecoder = decoderName.take(256)
    }
    override fun onAudioDecoderInitialized(eventTime: AnalyticsListener.EventTime, decoderName: String, initializedTimestampMs: Long, initializationDurationMs: Long) {
      if (lingoplayCurrentEvent(eventTime)) lingoplayAudioDecoder = decoderName.take(256)
    }
    override fun onRenderedFirstFrame(eventTime: AnalyticsListener.EventTime, output: Any, renderTimeMs: Long) {
      if (lingoplayCurrentEvent(eventTime) && lingoplayFirstFrameMs == null && lingoplaySourceAt > 0)
        lingoplayFirstFrameMs = (android.os.SystemClock.elapsedRealtime() - lingoplaySourceAt).coerceAtLeast(0)
    }`,
);
patchPlayer(
  '  fun prepare() {\n    availableVideoTracks = listOf()',
  `  fun prepare() {
    lingoplaySourceAt = android.os.SystemClock.elapsedRealtime()
    lingoplayFirstFrameMs = null
    lingoplayVideoDecoder = null
    lingoplayAudioDecoder = null
    availableVideoTracks = listOf()`,
);
writeFileSync(playerPath, playerSource);
