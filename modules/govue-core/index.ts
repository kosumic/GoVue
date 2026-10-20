import { requireNativeModule } from 'expo';

const native = requireNativeModule<{
  browserUrlPolicy?(url: string): string;
  state?(request: string): string;
  execute(request: string): Promise<string>;
  ai(request: string): Promise<string>;
  dictation?(request: string): string;
  supportedMediaFormats?(formats: string): Promise<string>;
  getSystemVolume?(): Promise<number>;
  getBrowserUserAgent?(): Promise<string>;
  takeBrowserLaunches?(): Promise<string[]>;
  observeDeviceOrientation?(enabled: boolean): Promise<void>;
  addListener?(
    event: string,
    listener: (sample?: Record<string, unknown>) => void,
  ): { remove(): void };
  setSystemVolume?(volume: number): Promise<void>;
  saveFile?(uri: string, name: string, mime: string): Promise<boolean>;
  galleryBytes?(url: string, key: string): Promise<string>;
  castDiscoverySnapshot?(enabled: boolean): Promise<string>;
  castNeedsLocalPermission?(): Promise<boolean>;
  configurePipControls?(raw: string): Promise<void>;
  pipBackgroundAudio?(videoId: string): Promise<boolean>;
  transcriptTrace?(
    requestId: string,
    stage: string,
    cookie: number,
    begin: boolean,
  ): boolean;
}>('GoVueCore');

export const browserUrlPolicy = (url: string): 'internal' | 'external' | 'blocked' => {
  const policy = native.browserUrlPolicy?.(url);
  return policy === 'internal' || policy === 'external' ? policy : 'blocked';
};

export const observeCoreUpdates = (channel: string, listener: () => void) =>
  native.addListener?.('coreUpdate', sample => { if (sample?.channel === channel) listener(); });

export function stateRequest<T>(command: Record<string, unknown>): T {
  if (!native.state) throw Error('Install an updated GoVue build to use shared app state.');
  const reply = JSON.parse(native.state(JSON.stringify(command)));
  if (reply?.ok !== true) throw Error(reply?.error || 'Could not update shared app state.');
  return reply.value as T;
}
export const browserRequest = stateRequest;

export function dictationRequest<T>(command: Record<string, unknown>): T {
  if (!native.dictation) throw Error('Install an updated GoVue build to use voice input.');
  const reply = JSON.parse(native.dictation(JSON.stringify(command)));
  if (reply?.ok !== true) throw Error(reply?.error || 'Could not update voice input.');
  return reply.value as T;
}

// Android async atrace slices are collected by Perfetto. Other platforms and
// older development clients can keep using the same transcript loading code.
export const transcriptTrace = (
  requestId: string,
  stage: string,
  cookie: number,
  begin: boolean,
) => native.transcriptTrace?.(requestId, stage, cookie, begin) ?? false;

export async function aiRequest<T>(
  command: Record<string, unknown>,
): Promise<T> {
  const reply = JSON.parse(await native.ai(JSON.stringify(command)));
  if (reply?.ok !== true)
    throw new Error(reply?.error || 'Could not complete the AI request.');
  return reply.value as T;
}

export const saveFileToDevice = (uri: string, name: string, mime: string) =>
  native.saveFile?.(uri, name, mime) ?? Promise.resolve(null);
export const getBrowserUserAgent = () =>
  native.getBrowserUserAgent?.() ?? Promise.resolve(null);
export const takeBrowserLaunches = () =>
  native.takeBrowserLaunches?.() ?? Promise.resolve([]);
export const observeBrowserLaunches = (listener: () => void) =>
  native.addListener?.('browserLaunch', listener);
export const setOrientationObservation = (enabled: boolean) =>
  native.observeDeviceOrientation?.(enabled) ?? Promise.resolve();
export const observePhysicalOrientation = (
  listener: (sample: { degrees: number; systemAutoRotate: boolean }) => void,
) =>
  native.addListener?.('deviceOrientation', (sample) => {
    if (
      sample &&
      typeof sample.degrees === 'number' &&
      Number.isInteger(sample.degrees) &&
      sample.degrees >= -1 &&
      sample.degrees < 360 &&
      typeof sample.systemAutoRotate === 'boolean'
    )
      listener({
        degrees: sample.degrees,
        systemAutoRotate: sample.systemAutoRotate,
      });
  });
export async function galleryBytes(
  url: string,
  key: string,
): Promise<{ status: number; location?: string }> {
  if (!native.galleryBytes) throw Error('Image loading is unavailable.');
  return JSON.parse(await native.galleryBytes(url, key));
}
export const castDiscoverySnapshot = (enabled: boolean) =>
  native.castDiscoverySnapshot?.(enabled) ?? Promise.resolve(null);
export const castNeedsLocalPermission = () =>
  native.castNeedsLocalPermission?.() ?? Promise.resolve(false);
export const configurePipControls = (state: Record<string, unknown>) =>
  native.configurePipControls?.(JSON.stringify(state)) ?? Promise.resolve();
export const pipBackgroundAudio = (videoId: string) =>
  native.pipBackgroundAudio?.(videoId) ?? Promise.resolve(false);
export const observePipActions = (
  listener: (sample: {
    action: string;
    videoId: string;
    handoff: boolean;
  }) => void,
) =>
  native.addListener?.('pipAction', (sample) => {
    if (
      sample &&
      typeof sample.videoId === 'string' &&
      /^[A-Za-z0-9_-]{11}$/.test(sample.videoId) &&
      typeof sample.action === 'string' &&
      [
        'audio',
        'toggle',
        'previous',
        'next',
        'foreground',
        'audio_complete',
        'audio_failed',
      ].includes(sample.action)
    )
      listener({
        action: sample.action,
        videoId: sample.videoId,
        handoff: sample.handoff === true,
      });
  });
export async function supportedMediaFormats(
  formats: {
    key: string;
    mimeType: string;
    width: number;
    height: number;
    fps: number;
  }[],
): Promise<string[]> {
  if (native.supportedMediaFormats)
    return JSON.parse(
      await native.supportedMediaFormats(JSON.stringify(formats)),
    );
  return formats
    .filter((f) => f.mimeType.includes('avc1') || f.mimeType.includes('mp4a'))
    .map((f) => f.key);
}
export const getSystemVolume = () =>
  native.getSystemVolume?.() ?? Promise.resolve(1);
export const setSystemVolume = (volume: number) =>
  native.setSystemVolume?.(volume) ?? Promise.resolve();
export async function coreRequest<T>(
  command: Record<string, unknown>,
): Promise<T> {
  const reply = JSON.parse(await native.execute(JSON.stringify(command)));
  if (reply?.ok !== true)
    throw new Error(reply?.error || 'Could not complete this request.');
  return reply.value as T;
}
