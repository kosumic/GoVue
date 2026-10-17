import { useEffect, useState } from 'react';
import { AppState } from 'react-native';
import type { VideoPlayer } from 'expo-video';
import { coreRequest } from '../../modules/lingoplay-core';

export type PlaybackDiagnostics = {
  nominalFps: number | null;
  renderedFps: number | null;
  expectedFps: number | null;
  videoCodec: string | null;
  audioCodec: string | null;
  videoDecoder: string | null;
  audioDecoder: string | null;
  width: number | null;
  height: number | null;
  videoBitrate: number | null;
  audioBitrate: number | null;
  audioSampleRate: number | null;
  audioChannels: number | null;
  renderedFrames: number;
  droppedFrames: number;
  droppedRatio: number | null;
  firstFrameMs: number | null;
  bufferingMs: number;
  bufferCount: number;
  playbackState: string;
  errorCode: string | null;
};
type DiagnosticsPlayer = VideoPlayer & {
  lingoplayPlaybackStats?: (
    uri: string,
  ) => Promise<Record<string, unknown> | null>;
};

export function usePlaybackDiagnostics(
  player: VideoPlayer,
  playback: { uri: string; videoId: string } | null,
) {
  const [diagnostics, setDiagnostics] = useState<PlaybackDiagnostics | null>(
    null,
  );
  const uri = playback?.uri,
    id = playback?.videoId;
  useEffect(() => {
    setDiagnostics(null);
    const sample = (player as DiagnosticsPlayer).lingoplayPlaybackStats;
    if (!uri || !id || !sample) return;
    let alive = true,
      busy = false,
      foreground = AppState.currentState === 'active';
    const refresh = async () => {
      if (!alive || busy || !foreground) return;
      busy = true;
      try {
        const raw = await sample.call(player, uri);
        if (!alive || !foreground || !raw) return;
        const next = await coreRequest<PlaybackDiagnostics>({
          op: 'player_diagnostics',
          videoId: id,
          sample: raw,
        });
        if (alive && foreground) setDiagnostics(next);
      } catch {
      } finally {
        busy = false;
      }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 1000);
    const app = AppState.addEventListener('change', (value) => {
      foreground = value === 'active';
      if (foreground) void refresh();
    });
    return () => {
      alive = false;
      clearInterval(timer);
      app.remove();
    };
  }, [player, uri, id]);
  return diagnostics;
}
