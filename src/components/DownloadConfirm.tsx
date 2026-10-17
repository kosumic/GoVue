import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { useEffect, useRef, useState } from 'react';
import {
  ActivityIndicator,
  Image,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  useWindowDimensions,
  View,
} from 'react-native';
import { ChevronRight, Download } from '../player/icons';
import { YouTubeSwitch as Switch } from './YouTubeSwitch';
import { PlayerSheet, SheetRow } from '../player/PlayerSheet';
import {
  downloadOptions,
  downloadPreferences,
  type DownloadConfig,
  type DownloadOptions,
} from '../core/downloads';
import type { MediaCatalog } from '../core/media';
import type { MediaResolverHandle } from '../media/MediaResolver';
import type { useDownloads } from '../media/downloads';
import type { QueueItem } from '../player/usePlayerSession';

export type DownloadTarget = { name: string; items: QueueItem[] };
export const formatBytes = (n: number) =>
  n >= 1024 ** 3
    ? `${(n / 1024 ** 3).toFixed(1)} GB`
    : n >= 1024 ** 2
      ? `${(n / 1024 ** 2).toFixed(1)} MB`
      : n > 0
        ? `${Math.ceil(n / 1024)} KB`
        : '—';
const languages = [
  ['en', 'English'],
  ['ja', 'Japanese'],
  ['ko', 'Korean'],
  ['zh', 'Chinese'],
  ['zh-Hant', 'Chinese (Traditional)'],
  ['de', 'German'],
  ['fr', 'French'],
  ['es', 'Spanish'],
  ['pt', 'Portuguese'],
  ['ru', 'Russian'],
  ['tr', 'Turkish'],
  ['hi', 'Hindi'],
  ['ar', 'Arabic'],
];
export function DownloadConfirm({
  target,
  onClose,
  resolver,
  currentCatalog,
  downloads,
  onSubmitted,
  onError,
}: {
  target: DownloadTarget | null;
  onClose: () => void;
  resolver: React.RefObject<MediaResolverHandle | null>;
  currentCatalog: MediaCatalog | null;
  downloads: ReturnType<typeof useDownloads>;
  onSubmitted: (added: number, skipped: number) => void;
  onError: (error: unknown) => void;
}) {
  const colors = useThemeColors();
  const screen = useWindowDimensions();
  const landscape = screen.width > screen.height;
  const s = useThemeStyles(makeStyles);
  const [config, setConfig] = useState<DownloadConfig | null>(null);
  const [catalog, setCatalog] = useState<MediaCatalog | null>(null);
  const [options, setOptions] = useState<DownloadOptions | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [more, setMore] = useState(false);
  const [picker, setPicker] = useState<'quality' | 'audio' | 'subtitle' | null>(
    null,
  );
  const [submitting, setSubmitting] = useState(false);
  const [selection, setSelection] = useState(new Set<string>());
  const [retry, setRetry] = useState(0);
  const version = useRef(0);
  const batch = (target?.items.length || 0) > 1;
  useEffect(() => {
    const epoch = ++version.current;
    const controller = new AbortController();
    setConfig(null);
    setCatalog(null);
    setOptions(null);
    setError('');
    setPicker(null);
    setMore(false);
    if (!target) return;
    setSelection(new Set(target.items.map((i) => i.videoId)));
    setLoading(!batch);
    void (async () => {
      const prefs = await downloadPreferences();
      if (version.current !== epoch) return;
      setConfig({
        ...prefs.lastConfig,
        videoKey: '',
        fileName: '',
        qualityHeight: prefs.lastConfig.qualityHeight || prefs.defaultQuality,
      });
      if (batch) return;
      const id = target.items[0]?.videoId;
      if (currentCatalog?.videoId === id && !retry) {
        setCatalog(currentCatalog);
        return;
      }
      if (!resolver.current)
        throw Error('The media resolver is still starting.');
      const result = await resolver.current.resolve(
        id,
        undefined,
        controller.signal,
      );
      if (version.current === epoch) setCatalog(result.catalog);
    })()
      .catch((e) => {
        if (version.current === epoch)
          setError(e instanceof Error ? e.message : 'Formats could not load.');
      })
      .finally(() => {
        if (version.current === epoch) setLoading(false);
      });
    return () => {
      version.current++;
      controller.abort();
    };
  }, [target, retry]);
  useEffect(() => {
    if (!catalog || !config) return;
    let active = true;
    void downloadOptions(catalog, config)
      .then((value) => {
        if (active) setOptions(value);
      })
      .catch((e) => {
        if (active)
          setError(e instanceof Error ? e.message : 'Options are unavailable.');
      });
    return () => {
      active = false;
    };
  }, [catalog, config]);
  const update = (values: Partial<DownloadConfig>) =>
    setConfig((v) => (v ? { ...v, ...values } : v));
  const quality =
    options?.qualities.find((q) => q.key === config?.videoKey) ||
    options?.qualities.find(
      (q) => q.height <= (config?.qualityHeight || 8640),
    ) ||
    options?.qualities.at(-1);
  const audio =
    options?.audioTracks?.find((a) => a.key === config?.audioKey)?.label ||
    'Default';
  const subtitle =
    options?.subtitles?.find(
      (t) => t.id === config?.subtitleKey || t.language === config?.subtitleKey,
    )?.label ||
    languages.find((l) => l[0] === config?.subtitleKey)?.[1] ||
    'Default';
  const submit = async () => {
    if (!target || !config) return;
    setSubmitting(true);
    try {
      const result = await downloads.enqueue(
        target.items.filter((i) => selection.has(i.videoId)),
        config,
        target.name,
      );
      onSubmitted(result.added || 0, result.skipped || 0);
    } catch (error) {
      onError(error);
    } finally {
      setSubmitting(false);
    }
  };
  return (
    <PlayerSheet
      visible={!!target}
      title={batch ? 'Download playlist' : 'Download'}
      onClose={onClose}
      showHeading={batch}
      footer={
        <View style={s.footer}>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={
              batch ? `Download ${selection.size} videos` : 'Start download'
            }
            disabled={
              submitting ||
              loading ||
              !config ||
              !selection.size ||
              (!batch && !options?.preview)
            }
            style={[
              s.download,
              (submitting ||
                loading ||
                !config ||
                !selection.size ||
                (!batch && !options?.preview)) &&
                s.disabled,
            ]}
            onPress={() => {
              void submit();
            }}
          >
            {submitting ? (
              <ActivityIndicator color={colors.inverted} />
            ) : (
              <Download size={18} color={colors.inverted} />
            )}
            <Text style={s.downloadText}>
              {batch ? `Download ${selection.size} videos` : 'Download'}
            </Text>
          </Pressable>
        </View>
      }
    >
      {picker && config && (
        <PlayerSheet
          visible={!!target && !!picker}
          title={
            picker === 'quality'
              ? 'Download quality'
              : picker === 'audio'
                ? 'Audio track'
                : 'Subtitles'
          }
          presentation="dialog"
          onClose={() => setPicker(null)}
          maxContentHeight={360}
        >
          {picker === 'quality' &&
            (config.audioOnly
              ? [
                  { key: 0, label: 'Auto' },
                  ...(options?.bitrates || [128000, 96000, 48000]).map((n) => ({
                    key: n,
                    label: `${Math.round(n / 1000)} kbps`,
                  })),
                ].map((a) => (
                  <SheetRow
                    radio
                    key={a.key}
                    title={a.label}
                    selected={config.audioBitrate === a.key}
                    onPress={() => {
                      update({ audioBitrate: a.key });
                      setPicker(null);
                    }}
                  />
                ))
              : (batch
                  ? [4320, 2160, 1440, 1080, 720, 480, 360, 240, 144].map(
                      (height) => ({
                        key: '',
                        height,
                        fps: 0,
                        label: `${height}p`,
                      }),
                    )
                  : options?.qualities || []
                ).map((q) => (
                  <SheetRow
                    radio
                    key={q.key || q.height}
                    title={q.label}
                    selected={
                      config.videoKey
                        ? q.key === config.videoKey
                        : batch
                          ? q.height === config.qualityHeight
                          : q.key === quality?.key
                    }
                    onPress={() => {
                      update({
                        videoKey: q.key,
                        qualityHeight: q.height,
                        qualityFps: q.fps,
                      });
                      setPicker(null);
                    }}
                  />
                )))}
          {picker === 'audio' &&
            [
              { key: '', label: 'Default' },
              ...(options?.audioTracks || []),
            ].map((a) => (
              <SheetRow
                radio
                key={a.key}
                title={a.label}
                selected={config.audioKey === a.key}
                onPress={() => {
                  update({ audioKey: a.key });
                  setPicker(null);
                }}
              />
            ))}
          {picker === 'subtitle' && (
            <>
              <SheetRow
                radio
                title="Off"
                selected={!config.includeSubtitle}
                onPress={() => {
                  update({ includeSubtitle: false });
                  setPicker(null);
                }}
              />
              {(batch
                ? languages.map(([id, label]) => ({
                    id,
                    label,
                    automatic: false,
                  }))
                : options?.subtitles || []
              ).map((t) => (
                <SheetRow
                  radio
                  key={t.id}
                  title={t.label}
                  detail={t.automatic ? 'Auto-generated' : undefined}
                  selected={
                    config.includeSubtitle && config.subtitleKey === t.id
                  }
                  onPress={() => {
                    update({ includeSubtitle: true, subtitleKey: t.id });
                    setPicker(null);
                  }}
                />
              ))}
            </>
          )}
        </PlayerSheet>
      )}
      {target && (
        <View style={s.body}>
          <View style={landscape && s.identityRow}>
            {!batch && (
              <Image
                source={{
                  uri: `https://i.ytimg.com/vi/${target.items[0].videoId}/hqdefault.jpg`,
                }}
                style={[s.thumbnail, landscape && { width: '42%' }]}
              />
            )}
            <View style={landscape && s.identityText}>
              <Text
                style={[s.title, landscape && { marginTop: 0 }]}
                numberOfLines={2}
              >
                {catalog?.title || target.name || target.items[0]?.title}
              </Text>
              <Text style={s.muted}>
                {batch
                  ? `${target.items.length} videos`
                  : catalog?.author || target.items[0]?.author}
              </Text>
            </View>
          </View>
          <View style={s.toggle}>
            {[
              [false, 'Video'],
              [true, 'Audio'],
            ].map(([audioOnly, label]) => {
              const selected = config?.audioOnly === audioOnly;
              return (
                <Pressable
                  key={String(label)}
                  accessibilityRole="radio"
                  accessibilityState={{ selected }}
                  style={s.toggleTarget}
                  onPress={() =>
                    update({
                      audioOnly: audioOnly as boolean,
                      attachmentsOnly: false,
                    })
                  }
                >
                  <View style={[s.toggleOption, selected && s.toggleSelected]}>
                    <Text style={[s.text, selected && s.dark]}>
                      {String(label)}
                    </Text>
                  </View>
                </Pressable>
              );
            })}
          </View>
          {loading && (
            <View style={s.busy}>
              <ActivityIndicator color={colors.ink} />
              <Text style={s.muted}>Loading formats…</Text>
            </View>
          )}
          <SelectionRow
            label="Download quality"
            value={
              config?.audioOnly
                ? config.audioBitrate
                  ? `${Math.round(config.audioBitrate / 1000)} kbps`
                  : 'Auto'
                : batch
                  ? `${config?.qualityHeight || 1080}p`
                  : quality?.label || '—'
            }
            disabled={!config || config.attachmentsOnly || loading}
            onPress={() => setPicker('quality')}
          />
          <SelectionRow
            label="Audio track"
            value={audio}
            disabled={!config || config.attachmentsOnly || loading}
            onPress={() => setPicker('audio')}
          />
          <SelectionRow
            label="Subtitles"
            value={config?.includeSubtitle ? subtitle : 'Off'}
            disabled={!config || loading}
            onPress={() => setPicker('subtitle')}
          />
          <View style={s.selection}>
            <Text style={s.text}>Size</Text>
            <Text style={s.muted}>
              {batch
                ? 'Calculated for each video'
                : formatBytes(options?.preview?.bytes || 0)}
            </Text>
          </View>
          {!!(error || options?.error) && (
            <>
              <Text accessibilityRole="alert" style={s.error}>
                {error || options?.error}
              </Text>
              <Pressable
                accessibilityRole="button"
                onPress={() => setRetry((v) => v + 1)}
                style={s.selection}
              >
                <Text style={s.text}>Retry</Text>
              </Pressable>
            </>
          )}
          <Pressable
            accessibilityRole="button"
            accessibilityState={{ expanded: more }}
            style={s.selection}
            onPress={() => setMore(!more)}
          >
            <Text style={s.text}>More options</Text>
            <ChevronRight
              size={20}
              color={colors.muted}
              style={{ transform: [{ rotate: more ? '90deg' : '0deg' }] }}
            />
          </Pressable>
          {more && config && (
            <>
              <SwitchRow
                label="Cover image"
                value={config.includeCover}
                onChange={(value) => update({ includeCover: value })}
              />
              <TextInput
                accessibilityLabel="Download filename"
                placeholder={
                  batch ? 'Filename template' : 'Filename (use video title)'
                }
                placeholderTextColor="#999"
                value={config.fileName}
                onChangeText={(fileName) => update({ fileName })}
                style={s.input}
                maxLength={180}
              />
              <SwitchRow
                label="Attachments only"
                value={config.attachmentsOnly}
                onChange={(value) =>
                  update({ attachmentsOnly: value, audioOnly: false })
                }
              />
            </>
          )}
          {batch && (
            <>
              <View style={s.selection}>
                <Text style={s.text}>Videos · {selection.size} selected</Text>
                <Pressable
                  accessibilityRole="button"
                  onPress={() =>
                    setSelection(
                      selection.size === target.items.length
                        ? new Set()
                        : new Set(target.items.map((i) => i.videoId)),
                    )
                  }
                >
                  <Text style={s.muted}>
                    {selection.size === target.items.length
                      ? 'Deselect all'
                      : 'Select all'}
                  </Text>
                </Pressable>
              </View>
              {target.items.map((item) => (
                <SheetRow
                  key={item.videoId}
                  title={item.title}
                  detail={item.author}
                  selected={selection.has(item.videoId)}
                  onPress={() =>
                    setSelection((old) => {
                      const next = new Set(old);
                      if (next.has(item.videoId)) next.delete(item.videoId);
                      else next.add(item.videoId);
                      return next;
                    })
                  }
                />
              ))}
            </>
          )}
        </View>
      )}
    </PlayerSheet>
  );
}
function SelectionRow({
  label,
  value,
  disabled,
  onPress,
}: {
  label: string;
  value: string;
  disabled?: boolean;
  onPress: () => void;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${label}: ${value}`}
      disabled={disabled}
      style={[s.selection, disabled && s.disabled]}
      onPress={onPress}
    >
      <Text style={s.text}>{label}</Text>
      <View style={s.value}>
        <Text numberOfLines={1} style={s.muted}>
          {value}
        </Text>
        <ChevronRight size={20} color={colors.muted} />
      </View>
    </Pressable>
  );
}
export function SwitchRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean;
  onChange: (value: boolean) => void;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  return (
    <View style={s.selection}>
      <Text style={s.text}>{label}</Text>
      <Switch
        accessibilityLabel={label}
        value={value}
        onValueChange={onChange}
      />
    </View>
  );
}
const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    body: { paddingHorizontal: 16 },
    identityRow: { flexDirection: 'row', alignItems: 'flex-start', gap: 12 },
    identityText: { flex: 1 },
    thumbnail: {
      width: '100%',
      aspectRatio: 16 / 9,
      borderRadius: 12,
      backgroundColor: colors.field,
    },
    title: {
      color: colors.ink,
      fontSize: 20,
      fontWeight: '500',
      lineHeight: 26,
      marginTop: 14,
    },
    muted: { color: colors.muted, fontSize: 13, flexShrink: 1 },
    text: { color: colors.ink, fontSize: 14 },
    dark: { color: colors.inverted },
    toggle: { flexDirection: 'row', gap: 8, marginVertical: 14 },
    toggleTarget: { minHeight: 48, justifyContent: 'center' },
    toggleOption: {
      borderRadius: 8,
      minHeight: 36,
      paddingHorizontal: 12,
      paddingVertical: 6,
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'center',
      gap: 8,
      backgroundColor: colors.raised,
    },
    toggleSelected: { backgroundColor: colors.ink },
    selection: {
      minHeight: 52,
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
      gap: 16,
    },
    value: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: 4,
      maxWidth: '65%',
    },
    input: {
      borderWidth: 1,
      borderColor: colors.outline,
      borderRadius: 8,
      color: colors.ink,
      fontSize: 14,
      padding: 12,
      marginVertical: 8,
    },
    busy: { padding: 20, gap: 12, flexDirection: 'row', alignItems: 'center' },
    error: { color: colors.error, fontSize: 13, lineHeight: 20 },
    footer: { paddingHorizontal: 16, paddingBottom: 8, paddingTop: 8 },
    download: {
      minHeight: 48,
      paddingHorizontal: 24,
      paddingVertical: 14,
      borderRadius: 20,
      backgroundColor: colors.ink,
      alignItems: 'center',
      justifyContent: 'center',
      flexDirection: 'row',
      gap: 8,
    },
    downloadText: { color: colors.inverted, fontSize: 14 },
    disabled: { opacity: 0.4 },
  });
