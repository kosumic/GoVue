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

// Auto-entry eligibility changes on pause, buffering, source changes and the
// headphones action. It must not replace the view already moved into PiP:
// otherwise exit can leave PlayerView in the window root instead of React's
// mounted VideoView. LiTube likewise retains the active surface across PiP.
patch(
  '  private var pipHelperFragment: PictureInPictureHelperFragment? = null',
  `  private var activePiPView = WeakReference<VideoView>(null)
  private var pipHelperFragment: PictureInPictureHelperFragment? = null`,
  'active PiP surface',
);
patch(
  `  override fun onPictureInPictureStart() {
    maybeWarnAboutAutoEnterViews()

    val candidate = currentPiPViewCandidate.get()`,
  `  override fun onPictureInPictureStart() {
    if (isInPiP) return
    maybeWarnAboutAutoEnterViews()

    val candidate = currentPiPViewCandidate.get()
    activePiPView = WeakReference(candidate)`,
  'retain PiP surface',
);
patch(
  `    currentPiPViewCandidate.get()?.let {
      layoutForPiPExit(it)
    }
    strongVideoViews.forEach {
      it.onStopPictureInPicture(currentPiPViewCandidate.get())
    }

    isInPiP = false`,
  `    val candidate = activePiPView.get()
    // Restore the root even if the video view was released during PiP.
    layoutForPiPExit(candidate)
    strongVideoViews.forEach {
      it.onStopPictureInPicture(candidate)
    }

    activePiPView.clear()
    isInPiP = false
    findAndSetupPipCandidate()`,
  'restore retained PiP surface',
);
patch(
  `  override fun onAppBackgrounded() {
    // At this point`,
  `  override fun onAppBackgrounded() {
    if (isInPiP) return
    // At this point`,
  'retain PiP owner on background callbacks',
);
patch(
  `    appContext.get()?.mainQueue?.launch {
      strongVideoViews.forEach {
        if (shouldPauseOnBackground(it)) {`,
  `    appContext.get()?.mainQueue?.launch {
      // A queued background callback can run after the user has returned and
      // pressed Play. It no longer owns playback in a resumed activity or PiP.
      val host = mainActivity.get() as? FragmentActivity
      if (isInPiP || host?.lifecycle?.currentState?.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED) == true) return@launch
      strongVideoViews.forEach {
        if (shouldPauseOnBackground(it)) {`,
  'ignore stale background pause',
);
patch(
  '    videoView?.addView(playerView)',
  `    videoView?.let {
      it.addView(playerView)
      // Re-measure the child against the current embedded/fullscreen bounds,
      // rather than retaining the pinned window's last dimensions.
      it.requestLayout()
    }`,
  'restore PiP layout',
);
patch(
  '  private var activePiPView = WeakReference<VideoView>(null)',
  `  private var activePiPPlayerView = WeakReference<View>(null)
  private var playerViewLayoutParams: ViewGroup.LayoutParams? = null
  private var activePiPView = WeakReference<VideoView>(null)`,
  'retain PiP child layout',
);
patch(
  `    val playerView = videoView.playerView
    val decorView = mainActivity.window.decorView`,
  `    val playerView = videoView.playerView
    activePiPPlayerView = WeakReference(playerView)
    playerViewLayoutParams = playerView.layoutParams
    val decorView = mainActivity.window.decorView`,
  'save embedded child layout',
);
patch(
  '    val playerView = videoView?.playerView',
  '    val playerView = videoView?.playerView ?: activePiPPlayerView.get()',
  'remove retained PiP child',
);
patch(
  '    rootView.removeView(playerView)',
  `    rootView.removeView(playerView)
    playerViewLayoutParams?.let { playerView?.layoutParams = it }`,
  'restore embedded child layout',
);
patch(
  `    hiddenPiPDialogs.clear()
  }`,
  `    hiddenPiPDialogs.clear()
    playerViewLayoutParams = null
    activePiPPlayerView.clear()
    rootView.requestLayout()
    // The React root was GONE while the window resized. Measure the restored
    // child after making that root visible, even when Yoga's bounds did not
    // change and React Native has no new layout to deliver.
    videoView?.let { view ->
      view.post {
        if (!isInPiP && view.playerView.parent === view) view.measureAndLayout()
      }
    }
  }`,
  'measure restored PiP child',
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
const videoEnabled = `      AsyncFunction("govueSetVideoEnabled") { ref: VideoPlayer, expectedUri: String, enabled: Boolean ->
        if (ref.player.currentMediaItem?.localConfiguration?.uri?.toString() != expectedUri) false
        else {
          ref.player.trackSelectionParameters = ref.player.trackSelectionParameters.buildUpon()
            .setTrackTypeDisabled(androidx.media3.common.C.TRACK_TYPE_VIDEO, !enabled).build()
          true
        }
      }.runOnQueue(Queues.MAIN)

${trackAnchor}`;
if (!moduleSource.includes(videoEnabled.slice(0, -trackAnchor.length))) {
  if (moduleSource.includes('AsyncFunction("govueSetVideoEnabled")'))
    throw Error(
      'Expo Video background renderer patch differs; review the adapter.',
    );
  if (!moduleSource.includes(trackAnchor))
    throw Error('Expo Video track property changed.');
  moduleSource = moduleSource.replace(trackAnchor, videoEnabled);
}
const selectTrack = `      AsyncFunction("govueSelectVideoTrack") { ref: VideoPlayer, expectedUri: String, trackId: String? ->
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
  if (moduleSource.includes('AsyncFunction("govueSelectVideoTrack")'))
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
const statsMethod = `      AsyncFunction("govuePlaybackStats") { ref: VideoPlayer, expectedUri: String ->
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
            "epoch" to ref.govueSourceAt,
            "sampledAt" to android.os.SystemClock.elapsedRealtime(),
            "nominalFps" to video?.frameRate?.takeIf { it.isFinite() && it > 0 },
            "refreshRate" to display?.refreshRate?.takeIf { it.isFinite() && it > 0 },
            "speed" to player.playbackParameters.speed,
            "videoCodec" to (video?.codecs ?: video?.sampleMimeType),
            "audioCodec" to (audio?.codecs ?: audio?.sampleMimeType),
            "videoDecoder" to ref.govueVideoDecoder,
            "audioDecoder" to ref.govueAudioDecoder,
            "width" to video?.width, "height" to video?.height,
            "videoBitrate" to video?.bitrate, "audioBitrate" to audio?.bitrate,
            "audioSampleRate" to audio?.sampleRate, "audioChannels" to audio?.channelCount,
            "renderedFrames" to (counters?.renderedOutputBufferCount ?: 0),
            "droppedFrames" to (counters?.droppedBufferCount ?: 0),
            "firstFrameMs" to ref.govueFirstFrameMs,
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
  if (moduleSource.includes('AsyncFunction("govuePlaybackStats")'))
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
const sourceTiming = `    android.os.Trace.beginSection("GoVue playback: prepare")
    android.os.Trace.endSection()
    govueSourceAt = android.os.SystemClock.elapsedRealtime()`;
const previousFirstFrameTiming = `        govueFirstFrameMs = (android.os.SystemClock.elapsedRealtime() - govueSourceAt).coerceAtLeast(0).also {
          android.os.Trace.beginSection("GoVue playback: first frame")
          android.os.Trace.endSection()
        }`;
const firstFrameTiming = previousFirstFrameTiming.replace(
  '          android.os.Trace.endSection()',
  '          android.os.Trace.endSection()\n          govueReleaseStartupLimit()',
);
if (playerSource.includes(previousFirstFrameTiming))
  playerSource = playerSource.replace(previousFirstFrameTiming, firstFrameTiming);
// Upgrade an already patched install as well as a fresh expo-video checkout.
if (!playerSource.includes(sourceTiming)) playerSource = playerSource.replace(
  '    govueSourceAt = android.os.SystemClock.elapsedRealtime()', sourceTiming,
);
if (!playerSource.includes(firstFrameTiming)) playerSource = playerSource.replace(
  '        govueFirstFrameMs = (android.os.SystemClock.elapsedRealtime() - govueSourceAt).coerceAtLeast(0)', firstFrameTiming,
);
patchPlayer(
  '  private val analyticsListener = object : AnalyticsListener {',
  `  var govueSourceAt: Long = 0L
  var govueFirstFrameMs: Long? = null
  var govueVideoDecoder: String? = null
  var govueAudioDecoder: String? = null

  private fun govueCurrentEvent(event: AnalyticsListener.EventTime): Boolean =
    !event.timeline.isEmpty && event.windowIndex in 0 until event.timeline.windowCount &&
      event.timeline.getWindow(event.windowIndex, Timeline.Window()).mediaItem.localConfiguration?.uri ==
        player.currentMediaItem?.localConfiguration?.uri

  private val analyticsListener = object : AnalyticsListener {
    override fun onVideoDecoderInitialized(eventTime: AnalyticsListener.EventTime, decoderName: String, initializedTimestampMs: Long, initializationDurationMs: Long) {
      if (govueCurrentEvent(eventTime)) govueVideoDecoder = decoderName.take(256)
    }
    override fun onAudioDecoderInitialized(eventTime: AnalyticsListener.EventTime, decoderName: String, initializedTimestampMs: Long, initializationDurationMs: Long) {
      if (govueCurrentEvent(eventTime)) govueAudioDecoder = decoderName.take(256)
    }
    override fun onRenderedFirstFrame(eventTime: AnalyticsListener.EventTime, output: Any, renderTimeMs: Long) {
      if (govueCurrentEvent(eventTime) && govueFirstFrameMs == null && govueSourceAt > 0)
${firstFrameTiming}
    }`,
);
patchPlayer(
  '  fun prepare() {\n    availableVideoTracks = listOf()',
  `  fun prepare() {
${sourceTiming}
    govueFirstFrameMs = null
    govueVideoDecoder = null
    govueAudioDecoder = null
    availableVideoTracks = listOf()`,
);
// The initial Auto rendition must not inherit an optimistic estimate from the
// preceding video. Rust selects a supported starting height; after the first
// frame the existing adaptive ladder can choose any supported resolution.
const previousStartupLimit = `  private var govueStartupHeight = 0
  private fun govueReleaseStartupLimit() {
    val height = player.videoFormat?.height ?: 0
    android.os.Trace.beginSection("GoVue playback: first height $height")
    android.os.Trace.endSection()
    if (govueStartupHeight > 0) {
      govueStartupHeight = 0
      player.trackSelectionParameters = player.trackSelectionParameters.buildUpon()
        .setMaxVideoSize(Int.MAX_VALUE, Int.MAX_VALUE).build()
      android.os.Trace.beginSection("GoVue playback: release startup limit")
      android.os.Trace.endSection()
    }
  }

  var govueSourceAt: Long = 0L`;
const startupLimit = previousStartupLimit.replace(
  '    val height = player.videoFormat?.height ?: 0',
  `    android.os.Trace.beginSection("GoVue playback: first position \${player.currentPosition.coerceAtLeast(0)}")
    android.os.Trace.endSection()
    val height = player.videoFormat?.height ?: 0`,
);
if (playerSource.includes(previousStartupLimit))
  playerSource = playerSource.replace(previousStartupLimit, startupLimit);
patchPlayer('  var govueSourceAt: Long = 0L', startupLimit);

// Correlate quality changes and buffering with the current source. Numeric
// positions/dimensions and an allowlisted codec label reveal no source URL.
patchPlayer(
  '    override fun onVideoInputFormatChanged(eventTime: AnalyticsListener.EventTime, format: Format, decoderReuseEvaluation: DecoderReuseEvaluation?) {',
  `    override fun onPlaybackStateChanged(eventTime: AnalyticsListener.EventTime, state: Int) {
      if (govueCurrentEvent(eventTime)) {
        android.os.Trace.beginSection("GoVue playback: state $state")
        android.os.Trace.endSection()
      }
    }
    override fun onVideoInputFormatChanged(eventTime: AnalyticsListener.EventTime, format: Format, decoderReuseEvaluation: DecoderReuseEvaluation?) {`,
);
patchPlayer(
  '    override fun onPlaybackStateChanged(eventTime: AnalyticsListener.EventTime, state: Int) {',
  `    override fun onIsPlayingChanged(eventTime: AnalyticsListener.EventTime, isPlaying: Boolean) {
      if (govueCurrentEvent(eventTime)) {
        android.os.Trace.beginSection("GoVue playback: playing $isPlaying")
        android.os.Trace.endSection()
      }
    }
    override fun onPlaybackStateChanged(eventTime: AnalyticsListener.EventTime, state: Int) {`,
);
patchPlayer(
  '      currentVideoTrack = availableVideoTracks.firstOrNull { it.format?.id == format.id }',
  `      currentVideoTrack = availableVideoTracks.firstOrNull { it.format?.id == format.id }
      if (govueCurrentEvent(eventTime)) {
        val codec = when (format.sampleMimeType) {
          "video/avc" -> "avc"
          "video/x-vnd.on2.vp9" -> "vp9"
          "video/av01" -> "av1"
          "video/hevc" -> "hevc"
          else -> "other"
        }
        val fps = format.frameRate.takeIf { it.isFinite() && it > 0 }?.toInt() ?: 0
        android.os.Trace.beginSection("GoVue playback: format \${format.width}x\${format.height} \${fps}fps $codec")
        android.os.Trace.endSection()
      }`,
);
patchPlayer('    val newSource = uncommittedSource', `    val newSource = uncommittedSource
    govueStartupHeight = newSource?.govueStartupHeight?.takeIf { it in 1..8640 } ?: 0
    player.trackSelectionParameters = player.trackSelectionParameters.buildUpon()
      .setMaxVideoSize(Int.MAX_VALUE, govueStartupHeight.takeIf { it > 0 } ?: Int.MAX_VALUE).build()`);
writeFileSync(playerPath, playerSource);

const videoSourcePath = 'node_modules/expo-video/android/src/main/java/expo/modules/video/records/VideoSource.kt';
let videoSource = readFileSync(videoSourcePath, 'utf8');
const sourceField = '  @Field var govueStartupHeight: Int = 0,\n';
if (!videoSource.includes(sourceField)) {
  const anchor = '  @Field var useCaching: Boolean = false,';
  if (!videoSource.includes(anchor)) throw Error('Expo Video source record changed; review startup selection.');
  videoSource = videoSource.replace(anchor, sourceField + anchor);
  writeFileSync(videoSourcePath, videoSource);
}
