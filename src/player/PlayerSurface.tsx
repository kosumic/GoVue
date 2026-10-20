import { useEffect, useMemo, useRef, useState } from 'react';
import {
  ActivityIndicator,
  Animated,
  BackHandler,
  PanResponder,
  Pressable,
  Share,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import { VideoView } from 'expo-video';
import { useEvent } from 'expo';
import { LinearGradient } from 'expo-linear-gradient';
import * as Brightness from 'expo-brightness';
import {
  ArrowLeft,
  Captions,
  CaptionsOff,
  Cast,
  CastLink,
  Download,
  Expand,
  Info,
  ListVideo,
  Lock,
  Maximize,
  Minimize,
  Pause,
  PictureInPicture2,
  Play,
  Plus,
  LoopNext,
  LoopRepeat,
  LoopPause,
  LoopShuffle,
  ResizeFit,
  ResizeFill,
  ResizeZoom,
  ResizeWidth,
  ResizeHeight,
  RotateCcw,
  Settings,
  Share2,
  SkipBack,
  SkipForward,
  Unlock,
  Volume2,
  X,
  Segments,
} from './icons';
import type { LucideIcon } from 'lucide-react-native';
import {
  ChevronDown,
  ChevronRight,
  FileText,
  Gauge,
  Sparkles,
} from 'lucide-react-native';
import {
  coreRequest,
  getSystemVolume,
  setSystemVolume,
} from '../../modules/govue-core';
import type { useMediaBrowser } from '../media/useMediaBrowser';
import type { usePlayerSession } from './usePlayerSession';
import { PlayerSheet, SheetRow, type MenuAnchor } from './PlayerSheet';
import {
  playerSpeedLabel,
  usePlayerChromeMeasure,
} from './PlayerChromeMeasure';
import { Range } from './Range';
import type { GestureKey } from '../core/media';
import { SubtitleEditor } from './SubtitleEditor';
import { CastPicker } from './CastPicker';
import { CaptionOverlay } from './CaptionOverlay';
import { QueueSheet } from './QueueSheet';
import { usePlaybackDiagnostics } from './usePlaybackDiagnostics';
import {
  PlaybackInfo,
  PlaybackInfoActions,
  playbackInfoRows,
} from './PlaybackInfo';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { useHaptic } from './Haptics';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { GestureOverlay, type GestureFeedback } from './GestureFeedback';
import { SponsorPrompt } from './SponsorPrompt';
import { SegmentsMenu } from './SegmentsMenu';
import { MarqueeTitle } from './MarqueeTitle';
import { usePlayerChapter } from './usePlayerChapter';

type Props = {
  media: ReturnType<typeof useMediaBrowser>;
  session: ReturnType<typeof usePlayerSession>;
  videoRef: React.RefObject<VideoView | null>;
  onDownload: (id?: string) => void;
  onDownloadQueue?: () => void;
  onAi: () => void;
  onPip: () => void;
  onBack: () => void;
  onRestore: () => void;
  onMiniClose: () => void;
  onError: (e: unknown) => void;
};
type Menu =
  | 'more'
  | 'speed'
  | 'quality'
  | 'audio'
  | 'subtitles'
  | 'style'
  | 'resize'
  | 'queue'
  | 'segments'
  | 'info'
  | 'cast'
  | null;
const time = (n: number) => {
  const v = Math.max(0, Math.floor(n || 0));
  return `${v >= 3600 ? `${Math.floor(v / 3600)}:` : ''}${v >= 3600 ? String(Math.floor(v / 60) % 60).padStart(2, '0') : Math.floor(v / 60)}:${String(v % 60).padStart(2, '0')}`;
};
const segmentColors: Record<string, string> = {
  sponsor: '#00d400',
  selfpromo: '#ffff00',
  interaction: '#cc00ff',
  intro: '#00ffff',
  outro: '#0202ed',
  preview: '#008fd6',
  music_offtopic: '#ff9900',
  filler: '#7300ff',
  poi_highlight: '#ff1684',
};
const segmentName = (category: string) =>
  ({
    sponsor: 'Sponsor',
    selfpromo: 'Self promotion',
    interaction: 'Interaction',
    intro: 'Intro',
    outro: 'Outro',
    preview: 'Preview',
    music_offtopic: 'Non-music section',
    filler: 'Filler',
  })[category] || category;
const segmentColor = (category: string) => segmentColors[category] || '#0f0';
function Control({
  icon: Icon,
  label,
  onPress,
  disabled = false,
  onLongPress,
  iconSize = 24,
  targetSize = 48,
  color = 'white',
  selected,
  circle = false,
}: {
  icon: LucideIcon;
  label: string;
  onPress: () => void;
  disabled?: boolean;
  onLongPress?: () => void;
  iconSize?: number;
  targetSize?: number;
  color?: string;
  selected?: boolean;
  circle?: boolean;
}) {
  const vibrate = useHaptic();
  const s = useThemeStyles(makeStyles);
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled, selected }}
      disabled={disabled}
      onPress={() => {
        vibrate();
        onPress();
      }}
      onLongPress={onLongPress}
      style={({ pressed }) => [
        s.control,
        { width: targetSize, height: targetSize },
        circle && s.controlCircle,
        pressed && s.controlPressed,
        (pressed || disabled) && s.dim,
      ]}
    >
      <Icon size={iconSize} color={color} />
    </Pressable>
  );
}
export function PlayerSurface({
  media,
  session,
  videoRef,
  onDownload,
  onDownloadQueue,
  onAi,
  onPip,
  onBack,
  onRestore,
  onMiniClose,
  onError,
}: Props) {
  const { player, settings, catalog } = media;
  const {
    fontScale,
    width: windowWidth,
    height: windowHeight,
  } = useWindowDimensions();
  const diagnostics = usePlaybackDiagnostics(player, media.playback);
  const colors = useThemeColors();
  const insets = useSafeAreaInsets();
  const vibrate = useHaptic();
  const s = useThemeStyles(makeStyles);
  const nativePlaying = useEvent(player, 'playingChange', {
    isPlaying: player.playing,
  }).isPlaying;
  const nativeStatus = useEvent(player, 'statusChange', {
    status: player.status,
  });
  const nativeProgress = useEvent(player, 'timeUpdate', {
    currentTime: player.currentTime,
    bufferedPosition: player.bufferedPosition,
    currentLiveTimestamp: player.currentLiveTimestamp,
    currentOffsetFromLive: player.currentOffsetFromLive,
  });
  const cast = media.cast.state;
  const remote = cast.active && cast.acknowledged && !cast.link;
  const playing = remote ? cast.phase === 'playing' : nativePlaying;
  const status = remote
    ? { status: cast.phase === 'buffering' ? 'loading' : 'readyToPlay' }
    : nativeStatus;
  const progress = remote
    ? {
        ...nativeProgress,
        currentTime: cast.position ?? 0,
        bufferedPosition: cast.position ?? 0,
      }
    : nativeProgress;
  const videoTrack = useEvent(player, 'videoTrackChange', {
    videoTrack: player.videoTrack,
  }).videoTrack;
  const [menu, setMenu] = useState<Menu>(null);
  const [anchor, setAnchor] = useState<MenuAnchor | undefined>();
  const subtitleAnchor = useRef<View | null>(null),
    speedAnchor = useRef<View | null>(null),
    qualityAnchor = useRef<View | null>(null),
    moreAnchor = useRef<View | null>(null);
  const [visible, setVisible] = useState(true);
  const [hint, setHint] = useState('');
  const [gestureFeedback, setGestureFeedback] =
    useState<GestureFeedback | null>(null);
  const [size, setSize] = useState({ width: 1, height: 1 });
  const surface = useRef<View | null>(null);
  const [chapterSheetHeight, setChapterSheetHeight] = useState<number>();
  const [preview, setPreview] = useState<number | null>(null);
  const [zoom, setZoom] = useState(1);
  const [ended, setEnded] = useState(false);
  useEffect(() => {
    setEnded(false);
  }, [media.playback?.videoId]);
  useEffect(() => {
    if (remote) {
      setEnded(cast.phase === 'ended');
      if (cast.phase === 'ended') setVisible(true);
    }
  }, [remote, cast.phase]);
  useEffect(() => {
    const end = player.addListener('playToEnd', () => {
      const owner = media.cast.stateRef.current;
      if (owner.active && owner.acknowledged && !owner.link) return;
      setEnded(true);
      setVisible(true);
    });
    const position = player.addListener('timeUpdate', ({ currentTime }) => {
      const owner = media.cast.stateRef.current;
      if (owner.active && owner.acknowledged && !owner.link) return;
      if (currentTime < player.duration - 0.5) setEnded(false);
    });
    return () => {
      end.remove();
      position.remove();
    };
  }, [player]);
  const togglePlayback = () => {
    if (ended) {
      setEnded(false);
      work(media.replay);
    } else work(playing ? media.pause : media.play);
  };
  const alpha = useRef(new Animated.Value(1)).current;
  const lastTap = useRef(0);
  const hold = useRef<number | null>(null);
  const tapSuppressedUntil = useRef(0);
  const singleTap = useRef<ReturnType<typeof setTimeout> | null>(null);
  const gestureLifetime = useRef(0);
  const doubleTaps = useRef(Promise.resolve());
  const gestureOwner = useRef(
    `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`,
  );
  const gestures = useRef({
    position: 0,
    volume: 1,
    brightness: 0.5,
    last: 0,
    pinch: 0,
    mode: '',
  });
  const gestureSequence = useRef(0);
  const current = useRef({ media, session, size, onBack });
  current.current = { media, session, size, onBack };
  const duration = remote
    ? cast.duration || 0
    : player.duration || catalog?.duration || 0;
  const position = Math.max(
    0,
    Math.min(duration || Infinity, preview ?? progress.currentTime),
  );
  const chapterStarts = useMemo(
    () => catalog?.chapters?.map((chapter) => chapter.start),
    [catalog?.chapters],
  );
  const chapterIndex = usePlayerChapter(
    media.playback?.videoId || '',
    catalog?.chapters,
    position,
    !session.mini && (visible || menu === 'segments'),
  );
  const chapter = chapterIndex === null ? null : catalog?.chapters?.[chapterIndex];
  const hasChapters = !!catalog?.chapters?.length;
  const compact = size.width < 48 * 6 + 24 + 128 * fontScale;
  const miniGap = Math.max(
    0,
    Math.round((50 * size.width) / (windowWidth < 600 ? 190 : 240) - 32),
  );
  const room = Math.max(
    0,
    Math.min(1, (size.width - 320) / 520, (size.height - 180) / 300),
  );
  const chrome = {
    icon: Math.round(24 + 4 * room),
    text: Math.round(14 + 2 * room),
    play: Math.round(64 + 16 * room),
    skip: Math.round(48 + 8 * room),
    gap: Math.max(12, Math.min(84, Math.round(size.width * 0.08))),
  };
  const busy =
    media.preparing || media.switchingQuality || status.status === 'loading';
  const queue = session.queue;
  const LoopIcon =
    queue.loopMode === 'repeat'
      ? LoopRepeat
      : queue.loopMode === 'shuffle'
        ? LoopShuffle
        : queue.loopMode === 'pause'
          ? LoopPause
          : LoopNext;
  const loopTitle = {
    next: 'Play next',
    repeat: 'Loop one',
    pause: 'Pause at end',
    shuffle: 'Shuffle',
  }[queue.loopMode];
  const decodedHeight = diagnostics?.height || videoTrack?.size?.height;
  const decodedFps = diagnostics?.nominalFps || videoTrack?.frameRate || 0;
  const activeQuality = decodedHeight
    ? `${decodedHeight}p${decodedFps > 30 ? Math.round(decodedFps) : ''}`
    : '';
  const quality = media.playback?.offline
    ? activeQuality
      ? activeQuality
      : 'Saved file'
    : media.playback?.qualityHeight
      ? `${media.playback.qualityHeight}p${(media.playback.qualityFps || 0) > 30 ? media.playback.qualityFps : ''}`
      : `Auto${activeQuality ? ` ${activeQuality}` : ''}`;
  const speedLabel = playerSpeedLabel(
    hold.current === null
      ? remote
        ? (cast.speed ?? settings.playbackSpeed)
        : settings.playbackSpeed
      : 2,
  );
  const durationLabel = catalog?.live ? 'LIVE' : time(duration);
  const timeSample = catalog?.live
    ? '88:88:88'
    : time(duration).replace(/\d/g, '8').padStart(5, '8');
  const bottomChrome = usePlayerChromeMeasure(
    { time: `${timeSample} / ${durationLabel}`, quality },
    chrome.text,
    fontScale,
    size.width,
    hasChapters,
  );
  const show = () => {
    setVisible(true);
  };
  const work = (fn: () => Promise<unknown>) => {
    void fn().catch(onError);
  };
  const flash = (text: string) => {
    setHint(text);
    show();
  };
  const open = (value: Menu) => {
    if (value === 'segments') {
      setAnchor(undefined);
      setChapterSheetHeight(undefined);
      setMenu(value);
      show();
      if (!session.fullscreen && windowHeight > windowWidth) {
        surface.current?.measureInWindow((_x, y, _width, height) => {
          setChapterSheetHeight(Math.max(200, windowHeight - y - height));
        });
      }
      return;
    }
    const ref =
      value === 'subtitles'
        ? subtitleAnchor
        : value === 'speed'
          ? speedAnchor
          : value === 'quality' && !bottomChrome.compact
            ? qualityAnchor
            : moreAnchor;
    if (
      value &&
      ['subtitles', 'speed', 'quality'].includes(value) &&
      ref.current
    ) {
      ref.current.measureInWindow((x, y, width, height) => {
        setAnchor({ x, y, width, height });
        setMenu(value);
        show();
      });
    } else {
      setAnchor(undefined);
      setMenu(value);
      show();
    }
  };
  useEffect(() => {
    Animated.timing(alpha, {
      toValue: visible ? 1 : 0,
      duration: 180,
      useNativeDriver: true,
    }).start();
  }, [visible, alpha]);
  useEffect(() => {
    if (
      !visible ||
      menu ||
      !playing ||
      preview !== null ||
      gestureFeedback ||
      status.status === 'error'
    )
      return;
    const timer = setTimeout(() => setVisible(false), 3000);
    return () => clearTimeout(timer);
  }, [visible, menu, playing, preview, !!gestureFeedback, status.status]);
  useEffect(() => {
    if (!hint) return;
    const timer = setTimeout(() => setHint(''), 1200);
    return () => clearTimeout(timer);
  }, [hint]);
  useEffect(() => {
    if (gestureFeedback?.kind !== 'double') return;
    const timer = setTimeout(() => setGestureFeedback(null), 500);
    return () => clearTimeout(timer);
  }, [gestureFeedback]);
  useEffect(() => {
    setMenu(null);
    setPreview(null);
    setGestureFeedback(null);
    ++gestureSequence.current;
    ++gestureLifetime.current;
    if (singleTap.current) clearTimeout(singleTap.current);
    setZoom(1);
    show();
  }, [media.playback?.videoId, media.playback?.offline]);
  useEffect(() => {
    setVisible(true);
    setGestureFeedback(null);
    ++gestureSequence.current;
    ++gestureLifetime.current;
    if (singleTap.current) clearTimeout(singleTap.current);
  }, [session.fullscreen, session.mini]);
  useEffect(() => {
    if (status.status === 'error') setVisible(true);
  }, [status.status]);
  useEffect(
    () => () => {
      if (singleTap.current) clearTimeout(singleTap.current);
      ++gestureLifetime.current;
    },
    [],
  );
  useEffect(() => {
    const sub = BackHandler.addEventListener('hardwareBackPress', () => {
      if (menu) {
        setMenu(null);
        return true;
      }
      if (session.locked) {
        session.setLocked(false);
        return true;
      }
      return false;
    });
    return () => sub.remove();
  }, [menu, session.locked]);
  useEffect(
    () => () => {
      if (hold.current !== null) player.playbackRate = hold.current;
    },
    [player],
  );
  const enabled = (name: string) =>
    current.current.media.settings[
      `gesture${name}${current.current.session.fullscreen ? 'Fullscreen' : 'Windowed'}` as GestureKey
    ] !== false;
  async function drag(dx: number, dy: number, x: number, commit: boolean) {
    const request = ++gestureSequence.current;
    const state = current.current;
    const gesture = gestures.current;
    const value = await coreRequest<{
      kind: string;
      delta: number;
      enabled: boolean;
      commit: boolean;
      target: number;
    }>({
      op: 'player_gesture',
      dx,
      dy,
      x,
      width: state.size.width,
      height: state.size.height,
      mode: gesture.mode,
      position: gesture.position,
      duration: player.duration || state.media.catalog?.duration || 0,
      volume: gesture.volume,
      brightness: gesture.brightness,
    });
    if (request !== gestureSequence.current) return;
    if (!gesture.mode) gesture.mode = value.kind;
    if (value.kind === 'seek' && enabled('Seek')) {
      const target = value.target;
      if (commit) {
        await current.current.media.seekTo(target);
        setPreview(null);
        setGestureFeedback(null);
      } else {
        setPreview(target);
        setGestureFeedback({
          kind: 'scrub',
          target,
          delta: target - gesture.position,
        });
      }
    } else if (value.kind === 'volume' && enabled('Volume')) {
      const volume = value.target;
      await setSystemVolume(volume);
      if (!commit && request === gestureSequence.current)
        setGestureFeedback({
          kind: 'edge',
          volume: true,
          percent: Math.round(volume * 100),
        });
    } else if (value.kind === 'brightness' && enabled('Brightness')) {
      const brightness = value.target;
      await Brightness.setBrightnessAsync(brightness);
      if (!commit && request === gestureSequence.current)
        setGestureFeedback({
          kind: 'edge',
          volume: false,
          percent: Math.round(brightness * 100),
        });
    } else if (
      value.kind === 'fullscreen' &&
      commit &&
      value.commit &&
      enabled('Fullscreen')
    ) {
      if (!value.enabled && state.media.playback?.offline) state.onBack();
      else state.session.setFullscreen(value.enabled);
    }
  }
  const responder = useRef(
    PanResponder.create({
      onMoveShouldSetPanResponder: (_e, g) =>
        !current.current.session.locked &&
        !current.current.session.mini &&
        (Math.abs(g.dx) > 12 ||
          Math.abs(g.dy) > 12 ||
          g.numberActiveTouches > 1),
      onPanResponderGrant: () => {
        if (singleTap.current) clearTimeout(singleTap.current);
        if (hold.current !== null) {
          current.current.media.holdSpeed(false);
          hold.current = null;
        }
        tapSuppressedUntil.current = Date.now() + 400;
        setGestureFeedback(null);
        gestures.current = {
          ...gestures.current,
          position: current.current.media.getPosition(),
          last: 0,
          pinch: 0,
          mode: '',
        };
        const active = gestures.current;
        void getSystemVolume()
          .then((value) => {
            if (gestures.current === active) active.volume = value;
          })
          .catch(() => {});
        void Brightness.getBrightnessAsync()
          .then((value) => {
            if (gestures.current === active) active.brightness = value;
          })
          .catch(() => {});
        show();
      },
      onPanResponderMove: (e, g) => {
        const points = e.nativeEvent.touches;
        if (points.length >= 2 && current.current.session.fullscreen) {
          const distance = Math.hypot(
            points[1].pageX - points[0].pageX,
            points[1].pageY - points[0].pageY,
          );
          if (gestures.current.pinch)
            setZoom((v) =>
              Math.max(1, Math.min(5, (v * distance) / gestures.current.pinch)),
            );
          gestures.current.pinch = distance;
          return;
        }
        if (Date.now() - gestures.current.last < 90) return;
        gestures.current.last = Date.now();
        void drag(g.dx, g.dy, e.nativeEvent.locationX - g.dx, false).catch(
          onError,
        );
      },
      onPanResponderRelease: (e, g) => {
        tapSuppressedUntil.current = Date.now() + 400;
        if (!gestures.current.pinch)
          void drag(g.dx, g.dy, e.nativeEvent.locationX - g.dx, true).catch(
            onError,
          );
        setPreview(null);
        setGestureFeedback(null);
      },
      onPanResponderTerminate: () => {
        ++gestureSequence.current;
        setPreview(null);
        setGestureFeedback(null);
        tapSuppressedUntil.current = Date.now() + 400;
      },
    }),
  ).current;
  const tap = (x: number) => {
    if (session.mini) {
      setVisible((value) => !value);
      return;
    }
    if (session.locked || Date.now() < tapSuppressedUntil.current) return;
    const now = Date.now();
    if (now - lastTap.current < 300 && enabled('DoubleTap')) {
      lastTap.current = 0;
      if (singleTap.current) clearTimeout(singleTap.current);
      singleTap.current = null;
      const lifetime = gestureLifetime.current;
      const videoId = media.playback?.videoId || catalog?.videoId;
      if (!videoId) return;
      doubleTaps.current = doubleTaps.current
        .catch(() => {})
        .then(async () => {
          if (gestureLifetime.current !== lifetime) return;
          const result = await coreRequest<{
            kind: 'toggle' | 'seek';
            target: number;
            accumulated: number;
          }>({
            op: 'player_double_tap',
            videoId,
            owner: gestureOwner.current,
            at: now,
            x,
            width: size.width,
            position: current.current.media.getPosition(),
            duration:
              player.duration || current.current.media.catalog?.duration || 0,
          });
          if (gestureLifetime.current !== lifetime) return;
          if (result.kind === 'toggle') {
            togglePlayback();
            return;
          }
          await current.current.media.seekTo(result.target);
          if (gestureLifetime.current !== lifetime) return;
          setHint('');
          setGestureFeedback({
            kind: 'double',
            forward: result.accumulated > 0,
            delta: result.accumulated,
            sequence: now,
          });
        })
        .catch(onError);
    } else {
      lastTap.current = now;
      if (singleTap.current) clearTimeout(singleTap.current);
      if (enabled('Tap'))
        singleTap.current = setTimeout(() => {
          singleTap.current = null;
          setVisible((value) => !value);
        }, 300);
    }
  };
  const change = (fn: () => Promise<unknown>) =>
    work(async () => {
      await fn();
      setMenu(null);
    });
  const title =
    menu === 'more'
      ? 'More'
      : menu === 'speed'
        ? 'Playback speed'
        : menu === 'quality'
          ? 'Quality'
          : menu === 'audio'
            ? 'Audio track'
            : menu === 'subtitles'
              ? 'Subtitles'
              : menu === 'style'
                ? 'Subtitle style'
                : menu === 'resize'
                  ? 'Resize'
                  : menu === 'queue'
                    ? 'Queue'
                    : menu === 'segments'
                      ? 'Segments'
                      : menu === 'cast'
                        ? 'Cast'
                        : 'Info';
  const infoRows = playbackInfoRows(diagnostics, {
    title: media.playback?.title || '',
    author: media.playback?.author || '',
    videoId: media.playback?.videoId || '',
    quality,
    duration: time(duration),
    live: !!catalog?.live,
    speed: remote ? (cast.speed ?? player.playbackRate) : player.playbackRate,
    state: status.status,
  });
  const aspect =
    videoTrack?.size?.width && videoTrack.size.height
      ? videoTrack.size.width / videoTrack.size.height
      : 16 / 9;
  const picture =
    settings.resizeMode === 'width'
      ? {
          position: 'absolute' as const,
          left: 0,
          top: (size.height - size.width / aspect) / 2,
          width: size.width,
          height: size.width / aspect,
        }
      : settings.resizeMode === 'height'
        ? {
            position: 'absolute' as const,
            top: 0,
            left: (size.width - size.height * aspect) / 2,
            height: size.height,
            width: size.height * aspect,
          }
        : StyleSheet.absoluteFill;
  return (
    <View
      ref={surface}
      collapsable={false}
      style={s.surface}
      onLayout={(e) => setSize(e.nativeEvent.layout)}
    >
      <VideoView
        ref={videoRef}
        player={player}
        style={[
          picture,
          { transform: [{ scale: zoom }], opacity: remote ? 0 : 1 },
        ]}
        nativeControls={false}
        contentFit={
          settings.resizeMode === 'fill' ||
          settings.resizeMode === 'width' ||
          settings.resizeMode === 'height'
            ? 'fill'
            : settings.resizeMode === 'zoom'
              ? 'cover'
              : 'contain'
        }
        surfaceType="textureView"
        allowsPictureInPicture={session.pipControls.available}
        startsPictureInPictureAutomatically={session.pipControls.autoEnter}
        onPictureInPictureStart={() => session.setPip(true)}
        onPictureInPictureStop={() => {
          session.setPip(false);
          // Re-read the page slot after the window has returned to the app.
          requestAnimationFrame(media.refreshPlayerLayout);
        }}
      />
      {bottomChrome.probe}
      <Pressable
        style={StyleSheet.absoluteFill}
        accessibilityLabel="Video gestures"
        onPress={(e) => tap(e.nativeEvent.locationX)}
        onLongPress={() => {
          if (!session.mini && !session.locked && enabled('LongPress')) {
            hold.current = player.playbackRate;
            tapSuppressedUntil.current = Date.now() + 400;
            media.holdSpeed(true);
            vibrate();
            setHint('');
            setGestureFeedback({ kind: 'speed' });
          }
        }}
        onPressOut={() => {
          if (hold.current !== null) {
            media.holdSpeed(false);
            hold.current = null;
            tapSuppressedUntil.current = Date.now() + 400;
            setGestureFeedback((value) =>
              value?.kind === 'speed' ? null : value,
            );
            setHint('');
          }
        }}
        {...responder.panHandlers}
      />
      {!!session.caption && !session.mini && (
        <CaptionOverlay
          text={session.caption}
          appearance={settings.subtitleStyle}
          width={size.width}
          height={size.height}
          controls={visible && !session.locked && !session.mini}
        />
      )}
      {!session.locked && !session.mini && (
        <Animated.View
          pointerEvents={visible ? 'box-none' : 'none'}
          accessibilityElementsHidden={!visible}
          importantForAccessibility={visible ? 'auto' : 'no-hide-descendants'}
          style={[StyleSheet.absoluteFill, { opacity: alpha }]}
        >
          <LinearGradient
            pointerEvents="none"
            colors={['#0009', 'transparent']}
            style={s.topScrim}
          />
          <LinearGradient
            pointerEvents="none"
            colors={['transparent', '#000b']}
            style={s.bottomScrim}
          />
          <View style={s.top}>
            <Control
              icon={session.fullscreen ? ChevronDown : ArrowLeft}
              iconSize={chrome.icon}
              label="Navigate back"
              onPress={onBack}
            />
            {!compact ? (
              <View style={s.metadata} pointerEvents="none">
                <MarqueeTitle
                  active={visible && !session.pip}
                  style={[
                    s.title,
                    { fontSize: chrome.text + 2, lineHeight: chrome.text + 8 },
                  ]}
                >
                  {media.playback?.title || catalog?.title || 'Loading video…'}
                </MarqueeTitle>
                <Text
                  numberOfLines={1}
                  style={[
                    s.author,
                    { fontSize: chrome.text - 2, lineHeight: chrome.text + 4 },
                  ]}
                >
                  {media.playback?.author || catalog?.author}
                </Text>
              </View>
            ) : (
              <View style={s.grow} />
            )}
            {!compact && (
              <>
                <Control
                  icon={ListVideo}
                  iconSize={chrome.icon}
                  label="Queue"
                  onPress={() => open('queue')}
                />
                <Control
                  icon={Segments}
                  iconSize={chrome.icon}
                  label={hasChapters ? 'Chapters' : 'Segments'}
                  onPress={() => open('segments')}
                />
                <Control
                  icon={LoopIcon}
                  iconSize={chrome.icon}
                  color={queue.loopMode === 'next' ? 'white' : '#ff0000'}
                  label={loopTitle}
                  onPress={() => work(() => session.action('loop'))}
                />
              </>
            )}
            <Control
              icon={Cast}
              iconSize={chrome.icon}
              label="Cast"
              disabled={media.playback?.offline === true}
              onPress={() => open('cast')}
            />
            <View collapsable={false} ref={subtitleAnchor}>
              <Control
                icon={
                  session.captions && settings.subtitleKey
                    ? Captions
                    : CaptionsOff
                }
                iconSize={chrome.icon}
                selected={!!session.captions && !!settings.subtitleKey}
                label="Subtitles"
                onPress={() => open('subtitles')}
                onLongPress={() => open('style')}
              />
            </View>
            <View collapsable={false} ref={moreAnchor}>
              <Control
                icon={Settings}
                iconSize={chrome.icon}
                label="More options"
                onPress={() => open('more')}
              />
            </View>
          </View>
          <View
            pointerEvents="box-none"
            style={[s.center, { marginTop: -chrome.play / 2 }]}
          >
            <Control
              icon={SkipBack}
              circle
              iconSize={chrome.icon}
              targetSize={chrome.skip}
              label="Previous"
              disabled={!queue.hasPrevious}
              onPress={() => work(() => session.advance('previous'))}
            />
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={ended ? 'Replay' : playing ? 'Pause' : 'Play'}
              onPress={() => {
                togglePlayback();
                show();
              }}
              style={({ pressed }) => [
                s.play,
                pressed && s.controlPressed,
                {
                  width: chrome.play,
                  height: chrome.play,
                  marginHorizontal: chrome.gap,
                },
              ]}
            >
              {busy ? (
                <ActivityIndicator color="white" size="large" />
              ) : ended ? (
                <RotateCcw size={chrome.play * 0.6} color="white" />
              ) : playing ? (
                <Pause fill="white" size={chrome.play * 0.6} color="white" />
              ) : (
                <Play fill="white" size={chrome.play * 0.6} color="white" />
              )}
            </Pressable>
            <Control
              icon={SkipForward}
              circle
              iconSize={chrome.icon}
              targetSize={chrome.skip}
              label="Next"
              disabled={!queue.hasNext}
              onPress={() => work(() => session.advance('next'))}
            />
          </View>
          <View style={s.bottom}>
            <View style={s.bottomRow}>
              {catalog?.live ? (
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel="Seek to live"
                  disabled={progress.currentOffsetFromLive == null}
                  onPress={() => work(() => media.seekLive())}
                  style={s.textButton}
                >
                  <Text style={[s.time, { fontSize: chrome.text }]}>
                    {time(position)} /{' '}
                    <Text
                      style={{
                        color: '#ff0000',
                        fontWeight: '500',
                      }}
                    >
                      LIVE
                    </Text>
                  </Text>
                </Pressable>
              ) : (
                <Text
                  numberOfLines={1}
                  accessibilityLabel={`${time(position)} of ${time(duration)}`}
                  style={[s.time, { fontSize: chrome.text }]}
                >
                  {time(position)}
                  <Text style={s.duration}>{` / ${time(duration)}`}</Text>
                </Text>
              )}
              {hasChapters ? (
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel={`Chapters${chapter?.title ? `, ${chapter.title}` : ''}`}
                  accessibilityState={{ expanded: menu === 'segments' }}
                  onPress={() => {
                    vibrate();
                    open('segments');
                  }}
                  style={({ pressed }) => [
                    s.chapterButton,
                    pressed && s.controlPressed,
                  ]}
                >
                  {bottomChrome.showChapterTitle ? (
                    <>
                      <Text
                        numberOfLines={1}
                        style={[s.chapterTitle, { fontSize: chrome.text }]}
                      >
                        {chapter?.title || 'Chapters'}
                      </Text>
                      <ChevronRight size={16} color="white" />
                    </>
                  ) : <Segments size={22} color="white" />}
                </Pressable>
              ) : <View style={s.grow} />}
              <Control
                icon={FileText}
                iconSize={chrome.icon}
                targetSize={44}
                label={session.transcript.open ? 'Hide transcript' : 'Show transcript'}
                selected={session.transcript.open}
                color={session.transcript.open ? '#ff0000' : 'white'}
                disabled={!media.playback}
                onPress={() => {
                  session.transcript.toggle();
                  show();
                }}
              />
              <Control
                icon={Sparkles}
                iconSize={chrome.icon}
                targetSize={44}
                label="Open AI chat"
                disabled={!media.playback}
                onPress={onAi}
              />
              {bottomChrome.showSpeed && (
                <View collapsable={false} ref={speedAnchor}>
                  <Control
                    icon={Gauge}
                    iconSize={chrome.icon}
                    targetSize={44}
                    label={`Playback speed, ${speedLabel}`}
                    onPress={() => open('speed')}
                  />
                </View>
              )}
              {!bottomChrome.compact && (
                <Pressable
                  ref={qualityAnchor}
                  accessibilityRole="button"
                  accessibilityLabel="Video quality"
                  accessibilityState={{
                    disabled: media.playback?.offline === true,
                  }}
                  disabled={media.playback?.offline === true}
                  style={s.textButton}
                  onPress={() => open('quality')}
                >
                  <Text style={[s.value, { fontSize: chrome.text }]}>
                    {quality}
                  </Text>
                </Pressable>
              )}
              <Control
                icon={session.fullscreen ? Minimize : Maximize}
                iconSize={chrome.icon}
                targetSize={44}
                label={session.fullscreen ? 'Exit fullscreen' : 'Fullscreen'}
                onPress={() => {
                  if (session.fullscreen && media.playback?.offline) onBack();
                  else session.setFullscreen(!session.fullscreen);
                }}
              />
            </View>
            <Range
              label="Seek video"
              timeBar
              trackColor="#ffffff4d"
              chapterStarts={chapterStarts}
              disabled={duration <= 0}
              onPreview={(target) => {
                setPreview(target);
                setGestureFeedback(
                  target === null
                    ? null
                    : {
                        kind: 'scrub',
                        target,
                        delta: target - progress.currentTime,
                      },
                );
                if (target !== null) {
                  setHint('');
                  show();
                }
              }}
              value={position}
              max={duration || 1}
              buffer={progress.bufferedPosition}
              onChange={(n) => {
                work(() => media.seekTo(n));
                show();
              }}
              markers={(media.sponsorSegments || []).map((v) => ({
                ...v,
                color: segmentColors[v.category] || '#0f0',
              }))}
            />
          </View>
        </Animated.View>
      )}
      {session.mini && (
        <Animated.View
          pointerEvents={visible ? 'box-none' : 'none'}
          accessibilityElementsHidden={!visible}
          importantForAccessibility={visible ? 'auto' : 'no-hide-descendants'}
          style={[StyleSheet.absoluteFill, s.miniShade, { opacity: alpha }]}
        >
          <View style={s.miniTop}>
            <Control
              icon={ListVideo}
              label="Queue"
              onPress={() => open('queue')}
            />
            <View style={s.grow} />
            <Control icon={X} label="Close mini-player" onPress={onMiniClose} />
          </View>
          <View style={s.miniCenter}>
            <Control
              icon={Maximize}
              targetSize={40}
              iconSize={20}
              label="Return to video"
              onPress={onRestore}
            />
          </View>
          <View style={[s.miniBottom, { gap: miniGap }]}>
            <Control
              icon={SkipBack}
              targetSize={30}
              iconSize={20}
              label="Previous"
              disabled={!queue.hasPrevious}
              onPress={() => work(() => session.advance('previous'))}
            />
            <Control
              icon={ended ? RotateCcw : playing ? Pause : Play}
              targetSize={34}
              label={ended ? 'Replay' : playing ? 'Pause' : 'Play'}
              onPress={togglePlayback}
            />
            <Control
              icon={SkipForward}
              targetSize={30}
              iconSize={20}
              label="Next"
              disabled={!queue.hasNext}
              onPress={() => work(() => session.advance('next'))}
            />
          </View>
        </Animated.View>
      )}
      {session.fullscreen &&
        !session.mini &&
        !session.pip &&
        (session.locked || visible) && (
          <View style={s.unlock}>
            <Control
              icon={session.locked ? Lock : Unlock}
              iconSize={chrome.icon}
              label={session.locked ? 'Unlock screen' : 'Lock screen'}
              onPress={() => {
                session.setLocked(!session.locked);
                show();
              }}
            />
          </View>
        )}
      {!session.mini &&
        !session.pip &&
        (media.sponsor.countdown !== null || media.sponsor.chip) &&
        !remote && (
          <View
            style={{
              position: 'absolute',
              right: Math.max(24, size.width * 0.12),
              bottom: 64,
            }}
          >
            <SponsorPrompt
              seconds={media.sponsor.countdown}
              highlight={media.sponsor.highlight}
              onSkip={() => work(() => media.sponsorControl('skip'))}
              onCancel={() => work(() => media.sponsorControl('cancel'))}
            />
          </View>
        )}
      {!session.mini && !session.locked && !session.pip && (
        <GestureOverlay
          feedback={gestureFeedback}
          width={size.width}
          height={size.height}
          inset={
            session.fullscreen
              ? Math.max(insets.top, insets.left, insets.right)
              : 0
          }
        />
      )}
      {!!hint && (
        <View pointerEvents="none" style={s.hint}>
          <Text style={s.hintText}>{hint}</Text>
        </View>
      )}
      {zoom > 1 && (
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Reset zoom"
          style={s.zoom}
          onPress={() => setZoom(1)}
        >
          <Text style={s.hintText}>Reset zoom · {zoom.toFixed(1)}×</Text>
        </Pressable>
      )}
      {status.status === 'error' && (
        <View style={s.failure}>
          <Text style={s.failureText}>Video could not load</Text>
          <Text style={s.failureDetail}>
            Refresh the source or return to the web player.
          </Text>
          <View style={s.failureActions}>
            <Pressable onPress={() => work(() => media.startNative())}>
              <Text style={s.action}>Retry</Text>
            </Pressable>
            <Pressable onPress={() => media.closePlayer()}>
              <Text style={s.action}>Web player</Text>
            </Pressable>
          </View>
        </View>
      )}
      <PlayerSheet
        visible={!!menu && menu !== 'cast' && menu !== 'segments'}
        title={title}
        anchor={anchor}
        scrollable={menu !== 'queue'}
        showCloseButton={menu !== 'more'}
        limitPlayerHeight
        onClose={() => setMenu(null)}
        presentation={
          menu === 'resize' || menu === 'audio' || menu === 'info'
            ? 'dialog'
            : 'sheet'
        }
        footer={
          menu === 'info' ? (
            <PlaybackInfoActions
              rows={infoRows}
              onCopy={() => flash('Copied')}
              onClose={() => setMenu(null)}
            />
          ) : undefined
        }
      >
        {menu === 'more' && (
          <>
            {!bottomChrome.showSpeed && (
              <SheetRow
                title="Playback speed"
                detail={speedLabel}
                onPress={() => open('speed')}
                icon={<Gauge size={24} />}
              />
            )}
            <SheetRow
              title="Resize"
              onPress={() => open('resize')}
              icon={<Expand color="white" size={24} />}
            />
            {bottomChrome.compact && (
              <SheetRow
                title="Quality"
                disabled={media.playback?.offline === true}
                onPress={() => open('quality')}
                icon={<Settings color="white" size={24} />}
              />
            )}
            <SheetRow
              title="Cast"
              disabled={media.playback?.offline === true}
              onPress={() => open('cast')}
              icon={<Cast color="white" size={24} />}
            />
            <SheetRow
              title="Picture-in-picture"
              disabled={!session.pipControls.available}
              onPress={() => {
                setMenu(null);
                onPip();
              }}
              icon={<PictureInPicture2 color="white" size={24} />}
            />
            <SheetRow
              title="Audio track"
              onPress={() => open('audio')}
              icon={<Volume2 color="white" size={24} />}
            />
            <SheetRow
              title="Subtitle style"
              onPress={() => open('style')}
              icon={<Captions color="white" size={24} />}
            />
            <SheetRow
              title="Info"
              onPress={() => open('info')}
              icon={<Info color="white" size={24} />}
            />
            {compact && (
              <SheetRow
                title="Queue"
                onPress={() => open('queue')}
                icon={<ListVideo color="white" size={24} />}
              />
            )}
            {compact && (
              <SheetRow
                title={hasChapters ? 'Chapters' : 'Segments'}
                onPress={() => open('segments')}
                icon={<Segments color="white" size={24} />}
              />
            )}
            {compact && (
              <SheetRow
                title={loopTitle}
                onPress={() => change(() => session.action('loop'))}
                icon={<LoopIcon color="white" size={24} />}
              />
            )}
            <SheetRow
              title="Download"
              onPress={() => {
                setMenu(null);
                onDownload();
              }}
              icon={<Download color="white" size={24} />}
            />
            <SheetRow
              title="Add to queue"
              onPress={() =>
                work(() => session.action('add', { item: media.playback }))
              }
              icon={<Plus color="white" size={24} />}
            />
            <SheetRow
              title="Share"
              onPress={() =>
                work(() =>
                  Share.share({
                    message: `https://www.youtube.com/watch?v=${media.playback?.videoId}`,
                  }),
                )
              }
              icon={<Share2 color="white" size={24} />}
            />
          </>
        )}
        {menu === 'speed' &&
          media.speeds.map((speed) => (
            <SheetRow
              key={speed}
              title={`${playerSpeedLabel(speed)}${speed === 1 ? ' (Normal)' : ''}`}
              selected={
                (remote ? cast.speed : settings.playbackSpeed) === speed
              }
              onPress={() => {
                media.setSpeed(speed);
                setMenu(null);
              }}
            />
          ))}
        {menu === 'quality' && (
          <>
            <SheetRow
              title="Auto"
              selected={!media.playback?.qualityHeight}
              disabled={media.switchingQuality}
              onPress={() => change(() => media.setQuality(0))}
            />
            {media.qualities.map((q) => (
              <SheetRow
                key={`${q.height}:${q.fps}`}
                title={q.label}
                selected={
                  media.playback?.qualityHeight === q.height &&
                  (!q.fps || media.playback.qualityFps === q.fps)
                }
                disabled={media.switchingQuality}
                onPress={() => change(() => media.setQuality(q.height, q.fps))}
              />
            ))}
            {media.switchingQuality && <ActivityIndicator color={colors.ink} />}
          </>
        )}
        {menu === 'audio' && (
          <>
            <SheetRow
              title="Default"
              selected={!settings.audioKey}
              onPress={() => change(() => media.setAudio(''))}
            />
            {catalog?.audioTracks?.map((track) => (
              <SheetRow
                key={track.key}
                title={track.label}
                selected={settings.audioKey === track.key}
                onPress={() => change(() => media.setAudio(track.key))}
              />
            ))}
          </>
        )}
        {menu === 'subtitles' && (
          <>
            {!catalog?.subtitles?.length && (
              <Text style={s.help}>
                No subtitles are available for this video.
              </Text>
            )}
            {catalog?.subtitles?.map((track) => (
              <SheetRow
                key={track.id}
                title={`${track.label}${track.automatic && !/auto.generated/i.test(track.label) ? ' (auto-generated)' : ''}`}
                selected={settings.subtitleKey === track.id}
                onPress={() => {
                  session.selectCaption(track.id);
                  setMenu(null);
                }}
              />
            ))}
            {session.captionBusy && <ActivityIndicator color="white" />}
            {!!session.captionError && (
              <Text style={s.help}>{session.captionError}</Text>
            )}
            <SheetRow title="Subtitle style" onPress={() => open('style')} />
          </>
        )}
        {menu === 'resize' &&
          (['fit', 'fill', 'zoom', 'width', 'height'] as const).map((mode) => (
            <SheetRow
              key={mode}
              title={
                {
                  fit: 'Fit',
                  fill: 'Fill',
                  zoom: 'Zoom',
                  width: 'Fixed width',
                  height: 'Fixed height',
                }[mode]
              }
              selected={settings.resizeMode === mode}
              showCheck={false}
              iconColor={
                settings.resizeMode === mode ? colors.ink : colors.muted
              }
              icon={
                mode === 'fit' ? (
                  <ResizeFit size={24} />
                ) : mode === 'fill' ? (
                  <ResizeFill size={24} />
                ) : mode === 'zoom' ? (
                  <ResizeZoom size={24} />
                ) : mode === 'width' ? (
                  <ResizeWidth size={24} />
                ) : (
                  <ResizeHeight size={24} />
                )
              }
              onPress={() => {
                media.updateSettings({ resizeMode: mode });
                setMenu(null);
              }}
            />
          ))}
        {menu === 'queue' && (
          <QueueSheet
            queue={queue}
            currentId={media.playback?.videoId}
            onPlay={(item) => {
              setMenu(null);
              session.go(item);
            }}
            onClose={() => setMenu(null)}
            onEnabled={(enabled) =>
              session.action('queue_enabled', { enabled })
            }
            onLoop={() => session.action('loop')}
            onClear={() => session.action('clear')}
            onRemove={(id) => session.action('remove', { id })}
            onMove={(id, offset) => session.action('move', { id, offset })}
            onDownloadAll={() => {
              setMenu(null);
              onDownloadQueue?.();
            }}
            onDownloadItem={(item) => {
              setMenu(null);
              onDownload(item.videoId);
            }}
            onError={onError}
          />
        )}
        {menu === 'style' && (
          <SubtitleEditor
            onClose={() => setMenu(null)}
            value={settings.subtitleStyle}
            videoId={media.playback?.videoId}
            onChange={(subtitleStyle) =>
              media.updateSettings({ subtitleStyle })
            }
          />
        )}
        {menu === 'info' && (
          <PlaybackInfo rows={infoRows} onCopy={() => flash('Copied')} />
        )}
      </PlayerSheet>
      {menu === 'segments' && (
        <SegmentsMenu
          visible={menu === 'segments'}
          videoId={media.playback?.videoId || ''}
          chapters={catalog?.chapters}
          segments={media.sponsorSegments}
          selected={chapterIndex}
          maxHeight={chapterSheetHeight}
          formatTime={time}
          categoryLabel={segmentName}
          categoryColor={segmentColor}
          onJump={(position, title) => {
            work(() => media.seekTo(position));
            flash(`Jumped to ${title}`);
            setMenu(null);
          }}
          onShare={(position) =>
            work(() => Share.share({
              message: `https://www.youtube.com/watch?v=${media.playback?.videoId}&t=${Math.floor(position)}`,
            }))
          }
          onClose={() => setMenu(null)}
        />
      )}
      {cast.active && (!cast.link || cast.linkConnected) && !session.pip && (
        <View style={s.castBanner}>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Manage casting"
            onPress={() => setMenu('cast')}
            style={s.castBannerButton}
          >
            {cast.link ? (
              <CastLink size={16} color="#fff" />
            ) : (
              <Cast size={16} color="#fff" />
            )}
            <Text style={s.castText}>
              {cast.link
                ? 'Shared player connected'
                : `${remote ? 'Casting to' : 'Connecting to'} ${cast.device}`}
            </Text>
          </Pressable>
          {cast.link && (
            <Control
              icon={X}
              targetSize={24}
              iconSize={16}
              label="Close link"
              onPress={() => work(() => media.cast.stop())}
            />
          )}
        </View>
      )}
      <CastPicker
        visible={menu === 'cast'}
        onClose={() => setMenu(null)}
        controller={media.cast}
        onError={onError}
      />
    </View>
  );
}
const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    surface: { flex: 1, backgroundColor: '#000', overflow: 'hidden' },
    castBanner: {
      position: 'absolute',
      top: '30%',
      alignSelf: 'center',
      flexDirection: 'row',
      alignItems: 'center',
      gap: 6,
      paddingHorizontal: 12,
      paddingVertical: 6,
      borderRadius: 12,
      backgroundColor: '#000c',
      maxWidth: '90%',
    },
    castText: { color: '#fff', fontSize: 12 },
    castBannerButton: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: 6,
      flexShrink: 1,
      minHeight: 24,
    },
    control: {
      width: 48,
      height: 48,
      alignItems: 'center',
      justifyContent: 'center',
    },
    dim: { opacity: 0.38 },
    controlCircle: { backgroundColor: '#0006', borderRadius: 100 },
    controlPressed: { backgroundColor: '#ffffff26', borderRadius: 100 },
    grow: { flex: 1 },
    top: {
      position: 'absolute',
      top: 0,
      left: 2,
      right: 4,
      flexDirection: 'row',
      alignItems: 'center',
      minHeight: 48,
    },
    metadata: { flex: 1, paddingHorizontal: 6 },
    title: {
      fontSize: 15,
      lineHeight: 21,
      color: 'white',
      includeFontPadding: false,
    },
    author: {
      fontSize: 11,
      lineHeight: 17,
      color: '#ffffffb3',
      includeFontPadding: false,
    },
    topScrim: { position: 'absolute', top: 0, left: 0, right: 0, height: 100 },
    bottomScrim: {
      position: 'absolute',
      bottom: 0,
      left: 0,
      right: 0,
      height: 100,
    },
    center: {
      position: 'absolute',
      top: '50%',
      alignSelf: 'center',
      flexDirection: 'row',
      alignItems: 'center',
      gap: 0,
    },
    play: {
      width: 64,
      height: 64,
      alignItems: 'center',
      justifyContent: 'center',
      borderRadius: 100,
      backgroundColor: '#0006',
    },
    bottom: { position: 'absolute', bottom: -12, left: 12, right: 12 },
    bottomRow: {
      flexDirection: 'row',
      alignItems: 'center',
      minHeight: 44,
    },
    time: {
      fontSize: 14,
      color: 'white',
      fontVariant: ['tabular-nums'],
      flexShrink: 1,
      includeFontPadding: false,
    },
    duration: { color: '#ffffffb3' },
    chapterButton: {
      flex: 1,
      minWidth: 44,
      minHeight: 44,
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'center',
      gap: 2,
      marginLeft: 6,
      paddingHorizontal: 4,
    },
    chapterTitle: {
      flexShrink: 1,
      color: 'white',
      fontSize: 14,
      includeFontPadding: false,
    },
    textButton: {
      minWidth: 48,
      height: 48,
      alignItems: 'center',
      justifyContent: 'center',
      paddingHorizontal: 6,
    },
    value: { color: 'white', fontSize: 13 },
    hint: {
      position: 'absolute',
      top: 4,
      alignSelf: 'center',
      backgroundColor: '#000c',
      borderRadius: 12,
      paddingHorizontal: 10,
      paddingVertical: 6,
    },
    hintText: { color: 'white', fontSize: 12 },
    zoom: {
      position: 'absolute',
      bottom: 80,
      right: 16,
      backgroundColor: '#000b',
      padding: 10,
      borderRadius: 18,
    },
    unlock: {
      position: 'absolute',
      left: 8,
      top: '50%',
      marginTop: -24,
    },
    failure: {
      position: 'absolute',
      top: 0,
      right: 0,
      bottom: 0,
      left: 0,
      backgroundColor: '#000d',
      alignItems: 'center',
      justifyContent: 'center',
      padding: 20,
      gap: 10,
    },
    failureText: { color: 'white', fontSize: 18 },
    failureDetail: { color: '#aaa', fontSize: 13 },
    failureActions: { flexDirection: 'row', gap: 28, padding: 14 },
    action: { color: '#fff', fontWeight: '600' },
    help: { color: colors.muted, fontSize: 14, lineHeight: 21, padding: 20 },
    queueRow: { flexDirection: 'row', alignItems: 'center', paddingLeft: 16 },
    queueItem: { flexDirection: 'row', alignItems: 'center', gap: 12, flex: 1 },
    thumb: { width: 100, height: 56, borderRadius: 6 },
    queueTitle: { color: colors.ink, fontSize: 14, lineHeight: 20 },
    active: { color: '#ff7373' },
    dot: { width: 12, height: 12, borderRadius: 6 },
    miniShade: { backgroundColor: '#0006' },
    miniTop: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      flexDirection: 'row',
    },
    miniCenter: {
      position: 'absolute',
      alignSelf: 'center',
      top: '50%',
      marginTop: -20,
    },
    miniBottom: {
      position: 'absolute',
      bottom: 4,
      left: 0,
      right: 0,
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'center',
    },
  });
