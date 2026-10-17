import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { useEffect, useState } from 'react';
import {
  ActivityIndicator,
  Alert,
  FlatList,
  Image,
  Modal,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import {
  ArrowLeft,
  Copy,
  Download,
  MoreVertical,
  Pause,
  Play,
  RotateCcw,
  Settings,
  Share2,
  Delete as Trash2,
  X,
} from '../player/icons';
import { coreRequest } from '../../modules/lingoplay-core';
import Clipboard from '@react-native-clipboard/clipboard';
import type { DownloadEntry } from '../core/downloads';
import type { useDownloads } from '../media/downloads';
import { IconButton } from './IconButton';
import { PlayerSheet, SheetRow } from '../player/PlayerSheet';
import { Range } from '../player/Range';
import { SwitchRow, formatBytes } from './DownloadConfirm';

type Destination = 'list' | 'settings' | 'history' | { batchId: string };
type DownloadView = {
  items: DownloadEntry[];
  total: number;
  active: number;
  completed: number;
  batch?: {
    name: string;
    total: number;
    completed: number;
    failed: number;
    pauseIds: string[];
    resumeIds: string[];
    ids: string[];
  } | null;
};
export function DownloadLibrary({
  visible,
  onClose,
  downloads,
  onPlay,
  onOpenVideo,
  onError,
}: {
  visible: boolean;
  onClose: () => void;
  downloads: ReturnType<typeof useDownloads>;
  onPlay: (entry: DownloadEntry) => void;
  onOpenVideo: (id: string) => void;
  onError: (error: unknown) => void;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const [destination, setDestination] = useState<Destination>('list');
  const [filter, setFilter] = useState('all');
  const [view, setView] = useState<DownloadView>({
    items: [],
    total: 0,
    active: 0,
    completed: 0,
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [batchBusy, setBatchBusy] = useState(false);
  const selected =
    downloads.entries.find((entry) => entry.id === selectedId) || null;
  const setSelected = (entry: DownloadEntry | null) =>
    setSelectedId(entry?.id || null);
  useEffect(() => {
    if (visible) {
      setDestination('list');
      setSelected(null);
    }
  }, [visible]);
  useEffect(() => {
    let active = true;
    void coreRequest<DownloadView>({
      op: 'downloads_view',
      entries: downloads.entries,
      batchId: typeof destination === 'object' ? destination.batchId : '',
      filter:
        destination === 'history'
          ? 'terminal'
          : typeof destination === 'object'
            ? 'all'
            : filter,
    })
      .then((result) => {
        if (active) setView(result);
      })
      .catch(onError);
    return () => {
      active = false;
    };
  }, [downloads.entries, filter, destination]);
  const run = (work: () => Promise<unknown>) => {
    void work().catch(onError);
  };
  const back = () =>
    destination === 'list' ? onClose() : setDestination('list');
  const title =
    destination === 'settings'
      ? 'Background & downloads'
      : destination === 'history'
        ? 'History'
        : typeof destination === 'object'
          ? 'Batch details'
          : 'Downloads';
  const remove = (entry: DownloadEntry) =>
    Alert.alert('Remove download?', entry.title, [
      { text: 'Cancel', style: 'cancel' },
      {
        text: 'Keep files',
        onPress: () => run(() => downloads.remove(entry, false)),
      },
      {
        text: 'Delete files',
        style: 'destructive',
        onPress: () => run(() => downloads.remove(entry, true)),
      },
    ]);
  const clearHistory = () =>
    Alert.alert(
      'Clear history?',
      'Remove completed, failed and cancelled download records?',
      [
        { text: 'Cancel', style: 'cancel' },
        {
          text: 'Keep files',
          onPress: () => run(() => downloads.clearHistory(false)),
        },
        {
          text: 'Delete files',
          style: 'destructive',
          onPress: () => run(() => downloads.clearHistory(true)),
        },
      ],
    );
  const batchWork = (work: () => Promise<unknown>) => {
    if (batchBusy) return;
    setBatchBusy(true);
    void work()
      .catch(onError)
      .finally(() => setBatchBusy(false));
  };
  const removeBatch = () => {
    const ids = view.batch?.ids || [];
    Alert.alert('Remove batch?', 'Remove these downloads from the manager?', [
      { text: 'Cancel', style: 'cancel' },
      {
        text: 'Keep files',
        onPress: () => batchWork(() => downloads.removeBatch(ids, false)),
      },
      {
        text: 'Delete files',
        style: 'destructive',
        onPress: () => batchWork(() => downloads.removeBatch(ids, true)),
      },
    ]);
  };
  return (
    <Modal visible={visible} animationType="slide" onRequestClose={back}>
      <SafeAreaView style={s.safe}>
        <View style={s.heading}>
          <IconButton
            icon={ArrowLeft}
            label="Back from downloads"
            onPress={back}
          />
          <Text accessibilityRole="header" style={s.title}>
            {title}
          </Text>
          {destination === 'list' && (
            <IconButton
              icon={Settings}
              label="Download settings"
              onPress={() => setDestination('settings')}
            />
          )}
        </View>
        {destination === 'settings' ? (
          <ScrollView contentContainerStyle={s.settings}>
            <SwitchRow
              label="Download over Wi-Fi only"
              value={downloads.preferences?.wifiOnly || false}
              onChange={(wifiOnly) =>
                run(() => downloads.configure({ wifiOnly }))
              }
            />
            <View style={s.divider} />
            <Text style={s.settingLabel}>Download quality</Text>
            <Text style={s.help}>
              Higher quality uses more data and storage
            </Text>
            {[144, 240, 360, 480, 720, 1080, 1440, 2160].map((height) => (
              <Pressable
                key={height}
                accessibilityRole="radio"
                accessibilityLabel={`${height}p`}
                accessibilityState={{
                  checked: downloads.preferences?.defaultQuality === height,
                }}
                style={s.radioRow}
                onPress={() =>
                  run(() =>
                    downloads.configure({
                      defaultQuality: height,
                      lastConfig: {
                        ...downloads.preferences!.lastConfig,
                        qualityHeight: height,
                        videoKey: '',
                      },
                    }),
                  )
                }
              >
                <View
                  style={[
                    s.radio,
                    downloads.preferences?.defaultQuality === height &&
                      s.radioSelected,
                  ]}
                >
                  {downloads.preferences?.defaultQuality === height && (
                    <View style={s.radioDot} />
                  )}
                </View>
                <Text style={s.settingText}>{height}p</Text>
              </Pressable>
            ))}
            <View style={s.divider} />
            <Text style={s.settingLabel}>
              Connections: {downloads.preferences?.maxConnections || 4}
            </Text>
            <Range
              color={colors.action}
              trackColor={colors.outline}
              label="Connections per download"
              value={downloads.preferences?.maxConnections || 4}
              max={16}
              min={1}
              onChange={(value) =>
                run(() =>
                  downloads.configure({ maxConnections: Math.round(value) }),
                )
              }
            />
            <View style={s.divider} />
            <SheetRow
              title="History"
              detail="Delete downloads"
              onPress={() => setDestination('history')}
            />
          </ScrollView>
        ) : (
          <>
            {destination === 'history' ? (
              <SheetRow title="Clear history" onPress={clearHistory} />
            ) : typeof destination === 'object' ? (
              <View style={s.batch}>
                <Text style={s.settingText}>
                  {view.batch?.name || 'Downloads'}
                </Text>
                <Text style={s.help}>
                  {view.batch?.completed || 0} of {view.batch?.total || 0}{' '}
                  completed
                  {view.batch?.failed ? ` · ${view.batch.failed} failed` : ''}
                </Text>
                <View style={s.track}>
                  <View
                    style={[
                      s.progress,
                      {
                        width: `${view.batch?.total ? (100 * view.batch.completed) / view.batch.total : 0}%`,
                      },
                    ]}
                  />
                </View>
                <View style={s.batchActions}>
                  {!!view.batch?.pauseIds.length && (
                    <Chip
                      label="Pause all"
                      selected={false}
                      disabled={batchBusy}
                      onPress={() =>
                        batchWork(async () => {
                          for (const id of view.batch?.pauseIds || [])
                            await downloads.pause(id, true);
                        })
                      }
                    />
                  )}
                  {!!view.batch?.resumeIds.length && (
                    <Chip
                      label="Resume all"
                      selected={false}
                      disabled={batchBusy}
                      onPress={() =>
                        batchWork(async () => {
                          for (const id of view.batch?.resumeIds || [])
                            await downloads.pause(id, false);
                        })
                      }
                    />
                  )}
                  <Chip
                    label="Remove batch"
                    selected={false}
                    disabled={batchBusy || !view.batch?.total}
                    onPress={removeBatch}
                  />
                  {batchBusy && <ActivityIndicator color={colors.ink} />}
                </View>
              </View>
            ) : (
              <View style={s.chips}>
                {[
                  ['all', 'All'],
                  ['active', 'In progress'],
                  ['complete', 'Completed'],
                ].map(([key, label]) => (
                  <Chip
                    key={key}
                    label={label}
                    selected={filter === key}
                    onPress={() => setFilter(key)}
                  />
                ))}
              </View>
            )}
            <View style={s.divider} />
            {!downloads.ready ? (
              <ActivityIndicator color={colors.ink} />
            ) : (
              <FlatList
                data={view.items}
                contentContainerStyle={
                  view.items.length ? undefined : s.emptyList
                }
                keyExtractor={(e) => e.id}
                ListEmptyComponent={
                  <View style={s.empty}>
                    <Download size={44} color="#666" />
                    <Text style={s.help}>No downloads yet</Text>
                  </View>
                }
                renderItem={({ item: entry }) => {
                  const active = [
                    'queued',
                    'resolving',
                    'downloading',
                    'paused',
                  ].includes(entry.state);
                  return (
                    <View style={s.row}>
                      <Pressable
                        accessibilityRole="button"
                        accessibilityLabel={`Details for ${entry.title}`}
                        onPress={() => setSelected(entry)}
                      >
                        <Image
                          source={{
                            uri: `https://i.ytimg.com/vi/${entry.videoId}/hqdefault.jpg`,
                          }}
                          style={s.thumbnail}
                        />
                      </Pressable>
                      <View style={s.grow}>
                        <Pressable
                          accessibilityRole="button"
                          onPress={() =>
                            entry.state === 'complete' &&
                            !entry.config?.attachmentsOnly
                              ? onPlay(entry)
                              : setSelected(entry)
                          }
                        >
                          <Text style={s.videoTitle} numberOfLines={2}>
                            {entry.title}
                          </Text>
                        </Pressable>
                        <Text style={s.help} numberOfLines={1}>
                          {entry.author}
                        </Text>
                        <Text style={s.help}>
                          {entry.state === 'complete'
                            ? 'Completed'
                            : entry.state === 'queued' &&
                                downloads.preferences?.wifiOnly
                              ? 'Waiting for Wi-Fi'
                              : entry.state.charAt(0).toUpperCase() +
                                entry.state.slice(1)}
                          {entry.label ? ` · ${entry.label}` : ''}
                        </Text>
                        {entry.total > 0 && entry.state !== 'complete' && (
                          <>
                            <View style={s.track}>
                              <View
                                style={[
                                  s.progress,
                                  {
                                    width: `${Math.min(100, (entry.received / entry.total) * 100)}%`,
                                  },
                                ]}
                              />
                            </View>
                            <Text style={s.help}>
                              {formatBytes(entry.received)} /{' '}
                              {formatBytes(entry.total)}
                            </Text>
                          </>
                        )}
                        {!!entry.error && (
                          <Text numberOfLines={2} style={s.error}>
                            {entry.error}
                          </Text>
                        )}
                      </View>
                      <View>
                        {active && (
                          <IconButton
                            icon={entry.state === 'paused' ? Play : Pause}
                            label={
                              entry.state === 'paused'
                                ? 'Resume download'
                                : 'Pause download'
                            }
                            onPress={() => run(() => downloads.pause(entry.id))}
                          />
                        )}
                        <IconButton
                          icon={MoreVertical}
                          label={`Download options for ${entry.title}`}
                          onPress={() => setSelected(entry)}
                        />
                      </View>
                    </View>
                  );
                }}
              />
            )}
          </>
        )}
        <PlayerSheet
          visible={!!selected}
          title={selected?.title || 'Download'}
          onClose={() => setSelected(null)}
        >
          {selected && (
            <>
              <Text style={s.detail}>
                {selected.author}
                {'\n'}
                {selected.label} · {selected.state}
                {'\n'}
                {formatBytes(selected.received)} / {formatBytes(selected.total)}
              </Text>
              {selected.state === 'complete' && (
                <>
                  {!selected.config?.attachmentsOnly && (
                    <SheetRow
                      title="Play"
                      icon={<Play color={colors.ink} />}
                      onPress={() => {
                        onPlay(selected);
                        setSelected(null);
                      }}
                    />
                  )}
                  {(selected.artifacts?.length
                    ? selected.artifacts
                    : [
                        {
                          file: selected.file,
                          mimeType: selected.mimeType,
                          label: selected.label,
                        },
                      ]
                  ).map((artifact) => (
                    <View key={artifact.file}>
                      <SheetRow
                        title={`Save ${artifact.label || 'file'}`}
                        icon={<Download color={colors.ink} />}
                        onPress={() =>
                          run(() => downloads.save(selected, artifact))
                        }
                      />
                      <SheetRow
                        title={`Share ${artifact.label || 'file'}`}
                        icon={<Share2 color={colors.ink} />}
                        onPress={() =>
                          run(() => downloads.share(selected, artifact))
                        }
                      />
                    </View>
                  ))}
                </>
              )}
              {['queued', 'resolving', 'downloading', 'paused'].includes(
                selected.state,
              ) && (
                <SheetRow
                  title={selected.state === 'paused' ? 'Resume' : 'Pause'}
                  icon={
                    selected.state === 'paused' ? (
                      <Play color={colors.ink} />
                    ) : (
                      <Pause color={colors.ink} />
                    )
                  }
                  onPress={() => {
                    run(() => downloads.pause(selected.id));
                    setSelected(null);
                  }}
                />
              )}
              {[
                'queued',
                'resolving',
                'downloading',
                'paused',
                'combining',
              ].includes(selected.state) ? (
                <SheetRow
                  title="Cancel download"
                  icon={<X color={colors.ink} />}
                  onPress={() => {
                    run(() => downloads.cancel(selected.id));
                    setSelected(null);
                  }}
                />
              ) : (
                <>
                  <SheetRow
                    title="Download again"
                    icon={<RotateCcw color={colors.ink} />}
                    onPress={() => {
                      run(() => downloads.retry(selected.id));
                      setSelected(null);
                    }}
                  />
                  <SheetRow
                    title="Remove"
                    icon={<Trash2 color={colors.ink} />}
                    onPress={() => {
                      remove(selected);
                      setSelected(null);
                    }}
                  />
                </>
              )}
              {!!selected.batchId &&
                downloads.entries.filter((e) => e.batchId === selected.batchId)
                  .length > 1 && (
                  <SheetRow
                    title="View batch"
                    onPress={() => {
                      setDestination({ batchId: selected.batchId! });
                      setSelected(null);
                    }}
                  />
                )}
              <SheetRow
                title="Copy video ID"
                icon={<Copy color={colors.ink} />}
                onPress={() => {
                  Clipboard.setString(selected.videoId);
                  setSelected(null);
                }}
              />
              <SheetRow
                title="Open on YouTube"
                onPress={() => {
                  onOpenVideo(selected.videoId);
                  setSelected(null);
                  onClose();
                }}
              />
            </>
          )}
        </PlayerSheet>
      </SafeAreaView>
    </Modal>
  );
}
function Chip({
  label,
  selected,
  disabled = false,
  onPress,
}: {
  label: string;
  selected: boolean;
  disabled?: boolean;
  onPress: () => void;
}) {
  const s = useThemeStyles(makeStyles);
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected }}
      disabled={disabled}
      onPress={onPress}
      style={[s.chip, selected && s.chipSelected]}
    >
      <Text style={[s.chipText, selected && s.chipSelectedText]}>{label}</Text>
    </Pressable>
  );
}
const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    safe: { flex: 1, backgroundColor: colors.background },
    batch: { paddingHorizontal: 16, paddingVertical: 12, gap: 8 },
    batchActions: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: 8,
      flexWrap: 'wrap',
    },
    heading: {
      height: 48,
      flexDirection: 'row',
      alignItems: 'center',
      paddingHorizontal: 8,
    },
    title: {
      flex: 1,
      color: colors.ink,
      fontSize: 20,
      fontWeight: '500',
      paddingLeft: 8,
    },
    chips: {
      flexDirection: 'row',
      gap: 8,
      paddingHorizontal: 16,
      paddingTop: 4,
      paddingBottom: 12,
      flexWrap: 'wrap',
    },
    chip: {
      backgroundColor: colors.raised,
      borderRadius: 8,
      minHeight: 32,
      justifyContent: 'center',
      paddingHorizontal: 12,
    },
    chipSelected: { backgroundColor: colors.ink },
    chipText: { color: colors.ink, fontSize: 13 },
    chipSelectedText: { color: colors.inverted },
    emptyList: { flexGrow: 1 },
    empty: {
      flex: 1,
      justifyContent: 'center',
      alignItems: 'center',
      gap: 12,
      padding: 24,
    },
    row: {
      flexDirection: 'row',
      gap: 12,
      padding: 16,
      borderBottomColor: colors.separator,
      borderBottomWidth: StyleSheet.hairlineWidth,
    },
    thumbnail: { width: 112, aspectRatio: 16 / 9, borderRadius: 8 },
    grow: { flex: 1 },
    videoTitle: {
      color: colors.ink,
      fontSize: 14,
      lineHeight: 20,
      fontWeight: '500',
    },
    help: { color: colors.muted, fontSize: 12, lineHeight: 18, marginTop: 4 },
    track: {
      height: 3,
      backgroundColor: colors.line,
      marginTop: 8,
      borderRadius: 2,
    },
    progress: { height: 3, backgroundColor: colors.ink, borderRadius: 2 },
    error: { color: colors.error, fontSize: 12, lineHeight: 18, marginTop: 4 },
    detail: { color: colors.muted, fontSize: 13, lineHeight: 20, padding: 16 },
    settings: { paddingHorizontal: 16, paddingBottom: 16 },
    divider: { height: StyleSheet.hairlineWidth, backgroundColor: '#42424266' },
    radioRow: {
      minHeight: 48,
      flexDirection: 'row',
      alignItems: 'center',
      gap: 16,
      paddingHorizontal: 14,
    },
    radio: {
      width: 20,
      height: 20,
      borderRadius: 10,
      borderWidth: 2,
      borderColor: colors.muted,
      justifyContent: 'center',
      alignItems: 'center',
    },
    radioSelected: { borderColor: colors.action },
    radioDot: {
      width: 10,
      height: 10,
      borderRadius: 5,
      backgroundColor: colors.action,
    },
    settingText: { fontSize: 14, color: colors.ink },
    settingLabel: {
      color: colors.ink,
      fontSize: 14,
      marginTop: 16,
      marginBottom: 4,
    },
  });
