import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { useEffect, useState } from 'react';
import {
  Alert,
  Linking,
  LayoutAnimation,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import {
  ArrowLeft,
  ChevronRight,
  Download,
  Hand,
  Play,
  BrandMark,
  PictureInPicture2,
  Reset,
  Settings,
  Shield,
  BackupRestore,
  Share2,
} from '../player/icons';
import * as FileSystem from 'expo-file-system/legacy';
import * as Sharing from 'expo-sharing';
import * as DocumentPicker from 'expo-document-picker';
import { Languages, Sparkles } from 'lucide-react-native';
import { coreRequest, saveFileToDevice } from '../../modules/govue-core';
import type { MediaSettings } from '../core/media';
import { Range } from '../player/Range';
import { SheetRow } from '../player/PlayerSheet';
import { YouTubeSwitch as Switch } from './YouTubeSwitch';
import { haptic } from '../player/Haptics';
import { useReducedMotion } from '../player/useReducedMotion';
import { AiSettings } from './AiSettings';
import { LanguageSettings } from './LanguageSettings';
import { version as appVersion } from '../../package.json';

type Node = {
  id?: string;
  key?: keyof MediaSettings;
  category?: string;
  title: string;
  summary?: string;
  kind?: string;
  children?: Node[];
};
const iconMap = {
  interface: Settings,
  player: Play,
  gesture: Hand,
  background: PictureInPicture2,
  sponsorblock: Shield,
  data: BackupRestore,
  downloads: Download,
  ai: Sparkles,
};
export function ExtensionSettings({
  page,
  settings,
  onSettings,
  onCategoryChange,
  onClose,
  onNavigate,
  onClearCache,
}: {
  page: 'settings' | 'ai' | 'data' | 'about';
  settings: MediaSettings;
  onSettings: (s: Partial<MediaSettings>) => void;
  onCategoryChange: (category: string, enabled: boolean) => void;
  onClose: () => void;
  onNavigate: (page: 'settings' | 'ai' | 'data' | 'about' | 'library') => void;
  onClearCache: () => void;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const { fontScale } = useWindowDimensions();
  const reducedMotion = useReducedMotion();
  const [nodes, setNodes] = useState<Node[]>([]);
  const [expanded, setExpanded] = useState<string[]>([]);
  const languagesOpen = expanded.includes('languages');
  const [notice, setNotice] = useState('');
  const [hapticPreview, setHapticPreview] = useState<number | null>(null);
  useEffect(() => setHapticPreview(null), [page]);
  const run = (fn: () => Promise<unknown>) => {
    void fn().catch((e) =>
      Alert.alert(
        'GoVue',
        e instanceof Error ? e.message : 'Please try again.',
      ),
    );
  };
  useEffect(() => {
    coreRequest<Node[]>({ op: 'media_settings_catalog' })
      .then(setNodes)
      .catch(() => {});
  }, []);
  async function exportFile(name: string, data: unknown, share = false) {
    const path = `${FileSystem.cacheDirectory}${name}`;
    await FileSystem.writeAsStringAsync(path, JSON.stringify(data, null, 2));
    if (!share) {
      const saved = await saveFileToDevice(path, name, 'application/json');
      if (saved !== null) return saved;
    }
    await Sharing.shareAsync(path, {
      mimeType: 'application/json',
      dialogTitle: name,
    });
  }
  async function exportDiagnostics(share: boolean) {
    return exportFile(
      'govue-diagnostics.json',
      {
        app: 'GoVue',
        version: appVersion,
        core: await coreRequest({ op: 'info' }),
        transport: await coreRequest({ op: 'media_transport_status' }),
      },
      share,
    );
  }
  function toggleSection(key: string) {
    const open = expanded.includes(key);
    if (!reducedMotion)
      LayoutAnimation.configureNext({
        duration: open ? 220 : 300,
        create: { type: 'easeInEaseOut', property: 'opacity' },
        update: {
          type: open ? 'easeInEaseOut' : 'spring',
          springDamping: 1,
        },
        delete: { type: 'easeInEaseOut', property: 'opacity' },
      });
    setExpanded((v) =>
      v.includes(key) ? v.filter((i) => i !== key) : [...v, key],
    );
  }
  const render = (node: Node, depth = 0): React.ReactNode => {
    const key = node.id || node.key || node.title;
    if (node.kind === 'nav') {
      const Icon = iconMap[node.id as keyof typeof iconMap] || Settings;
      return (
        <SheetRow
          key={key}
          title={node.title}
          detail={node.summary}
          section
          icon={<Icon color={colors.ink} size={24} />}
          trailing={<ChevronRight color={colors.muted} size={24} />}
          onPress={() =>
            onNavigate(node.id === 'downloads' ? 'library' : node.id === 'ai' ? 'ai' : 'data')
          }
        />
      );
    }
    if (node.children) {
      const open = expanded.includes(key);
      const Icon = iconMap[node.id as keyof typeof iconMap];
      return (
        <View key={key}>
          <View style={{ paddingLeft: Math.max(0, depth - 1) * 12 }}>
            <SheetRow
              title={node.title}
              section
              expanded={open}
              icon={Icon ? <Icon color={colors.ink} size={24} /> : undefined}
              trailing={
                <ChevronRight
                  size={24}
                  color={colors.muted}
                  style={{ transform: [{ rotate: open ? '90deg' : '0deg' }] }}
                />
              }
              onPress={() => toggleSection(key)}
            />
          </View>
          {open && node.children.map((child) => render(child, depth + 1))}
        </View>
      );
    }
    if (node.kind === 'slider')
      return (
        <View key={key} style={s.slider}>
          <Text style={s.label}>{node.title}</Text>
          <Text style={s.detail}>
            {(hapticPreview ?? settings.hapticStrength)
              ? `${hapticPreview ?? settings.hapticStrength}%`
              : 'Off'}
          </Text>
          <Range
            color={colors.action}
            trackColor={colors.outline}
            label={node.title}
            value={settings.hapticStrength}
            max={100}
            onPreview={(value) =>
              setHapticPreview(value === null ? null : Math.round(value))
            }
            onChange={(v) => {
              onSettings({ hapticStrength: Math.round(v) });
              haptic(Math.round(v));
            }}
          />
        </View>
      );
    if (node.kind === 'category' && node.category) {
      const category = node.category;
      const selected = settings.sponsorBlock && settings.categories.includes(category);
      return (
        <SheetRow
          key={key}
          title={node.title}
          onPress={() => onCategoryChange(category, !selected)}
          trailing={
            <Switch
              accessibilityLabel={node.title}
              value={selected}
              onValueChange={(enabled) => onCategoryChange(category, enabled)}
            />
          }
        />
      );
    }
    if (!node.key) return null;
    const field = node.key;
    const enabled = settings[field] === true;
    return (
      <View key={key} style={{ paddingLeft: Math.max(0, depth - 1) * 12 }}>
        <SheetRow
          title={node.title}
          detail={node.summary}
          onPress={() => onSettings({ [field]: !enabled })}
          trailing={
            <Switch
              accessibilityLabel={node.title}
              value={enabled}
              onValueChange={(value) => onSettings({ [field]: value })}
            />
          }
        />
      </View>
    );
  };
  return (
    <View style={s.page}>
      <View
        style={[s.heading, { minHeight: Math.max(48, 26 * fontScale + 20) }]}
      >
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Navigate back"
          style={s.back}
          onPress={onClose}
        >
          <ArrowLeft color={colors.ink} size={24} />
        </Pressable>
        <Text accessibilityRole="header" style={s.title}>
          {page === 'settings'
            ? 'Extensions'
            : page === 'ai'
              ? 'AI'
              : page === 'data'
                ? 'Backup & restore'
                : 'About'}
        </Text>
        {page === 'settings' && (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Reset extensions"
            style={s.back}
            onPress={() =>
              Alert.alert(
                'Reset extensions?',
                'Restore all extension preferences to their defaults.',
                [
                  { text: 'Cancel', style: 'cancel' },
                  {
                    text: 'Reset',
                    onPress: () =>
                      run(async () => {
                        const next = await coreRequest<MediaSettings>({
                          op: 'media_settings',
                          reset: true,
                        });
                        onSettings(next);
                      }),
                  },
                ],
              )
            }
          >
            <Reset color={colors.ink} size={24} />
          </Pressable>
        )}
      </View>
      {page === 'ai' ? <AiSettings /> : <ScrollView contentContainerStyle={s.body} keyboardShouldPersistTaps="handled">
        {page === 'settings' && (
          <>
            <SheetRow
              title="Languages"
              section
              expanded={languagesOpen}
              icon={<Languages color={colors.ink} size={24} />}
              trailing={<ChevronRight size={24} color={colors.muted} style={{ transform: [{ rotate: languagesOpen ? '90deg' : '0deg' }] }} />}
              onPress={() => toggleSection('languages')}
            />
            {languagesOpen && <View style={s.languages}>
              <LanguageSettings />
            </View>}
            {nodes.map((node) => render(node))}
          </>
        )}
        {page === 'data' && (
          <>
            <SheetRow
              title="Export settings"
              detail="Save current settings to a JSON file"
              icon={<Share2 size={24} color={colors.ink} />}
              onPress={() =>
                run(async () =>
                  exportFile(
                    'govue-settings.json',
                    await coreRequest({ op: 'media_settings_export' }),
                  ),
                )
              }
            />
            <SheetRow
              title="Import settings"
              detail="Restore settings from a backup file"
              icon={<Download size={24} color={colors.ink} />}
              onPress={() =>
                run(async () => {
                  const result = await DocumentPicker.getDocumentAsync({
                    type: 'application/json',
                    copyToCacheDirectory: true,
                  });
                  if (result.canceled) return;
                  const file = result.assets[0];
                  if ((file.size || 0) > 1000000)
                    throw Error('Settings backup is too large.');
                  const data = JSON.parse(
                    await FileSystem.readAsStringAsync(file.uri),
                  );
                  Alert.alert(
                    'Import settings?',
                    'This replaces matching extension and player preferences.',
                    [
                      { text: 'Cancel', style: 'cancel' },
                      {
                        text: 'Import',
                        onPress: () =>
                          run(async () => {
                            onSettings(
                              await coreRequest<MediaSettings>({
                                op: 'media_settings_import',
                                data,
                              }),
                            );
                            setNotice('Settings imported.');
                          }),
                      },
                    ],
                  );
                })
              }
            />
          </>
        )}
        {page === 'about' && (
          <>
            <View style={s.brand}>
              <View style={s.mark}>
                <BrandMark size={108} color={colors.ink} />
              </View>
              <Text style={s.name}>GoVue</Text>
              <Text style={[s.detail, { marginTop: 0 }]}>
                {appVersion} · LiTube port
              </Text>
            </View>
            <SheetRow
              labelColor={colors.action}
              title="Source code"
              onPress={() =>
                run(() =>
                  Linking.openURL('https://github.com/kosumic/GoVue'),
                )
              }
            />
            <View style={s.line} />
            <SheetRow
              labelColor={colors.action}
              title="Check for stable updates"
              onPress={() =>
                run(() =>
                  Linking.openURL(
                    'https://github.com/kosumic/GoVue/releases/latest',
                  ),
                )
              }
            />
            <SheetRow
              labelColor={colors.action}
              title="Open latest stable release"
              onPress={() =>
                run(() =>
                  Linking.openURL(
                    'https://github.com/kosumic/GoVue/releases/latest',
                  ),
                )
              }
            />
            <SheetRow
              labelColor={colors.action}
              title="Clear cache"
              onPress={() =>
                Alert.alert(
                  'Clear cache?',
                  'Remove cached web resources from YouTube.',
                  [
                    { text: 'Cancel', style: 'cancel' },
                    {
                      text: 'Clear',
                      onPress: () => {
                        onClearCache();
                        setNotice('Cache cleared.');
                      },
                    },
                  ],
                )
              }
            />
            <SheetRow
              labelColor={colors.action}
              title="Export diagnostic logs"
              onPress={() => run(() => exportDiagnostics(true))}
            />
            <SheetRow
              labelColor={colors.action}
              title="Save diagnostic logs"
              onPress={() => run(() => exportDiagnostics(false))}
            />
            <SheetRow
              labelColor={colors.action}
              title="LiTube"
              detail="Based on HydeYYHH/Litube · GPL-3.0"
              onPress={() =>
                run(() => Linking.openURL('https://github.com/HydeYYHH/Litube'))
              }
            />
            <SheetRow
              labelColor={colors.action}
              title="License"
              detail="GNU General Public License v3.0"
              onPress={() =>
                run(() =>
                  Linking.openURL(
                    'https://github.com/kosumic/GoVue/blob/master/LICENSE',
                  ),
                )
              }
            />
          </>
        )}
        {!!notice && (
          <Text accessibilityLiveRegion="polite" style={s.notice}>
            {notice}
          </Text>
        )}
      </ScrollView>}
    </View>
  );
}
const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    page: { flex: 1, backgroundColor: colors.background },
    heading: {
      flexDirection: 'row',
      alignItems: 'center',
      paddingHorizontal: 4,
    },
    back: {
      width: 48,
      height: 48,
      alignItems: 'center',
      justifyContent: 'center',
    },
    title: {
      flex: 1,
      color: colors.ink,
      fontSize: 20,
      fontWeight: '500',
      paddingLeft: 4,
    },
    body: {
      paddingBottom: 32,
      width: '100%',
      maxWidth: 720,
      alignSelf: 'center',
    },
    languages: { paddingHorizontal: 16, paddingTop: 16, paddingBottom: 8 },
    label: { color: colors.ink, fontSize: 14, lineHeight: 20 },
    detail: { color: colors.muted, fontSize: 12, lineHeight: 18, marginTop: 3 },
    slider: { paddingHorizontal: 16, paddingTop: 8 },
    brand: { alignItems: 'center', paddingVertical: 24 },
    mark: {
      width: 72,
      height: 72,
      overflow: 'hidden',
      alignItems: 'center',
      justifyContent: 'center',
    },
    name: { color: colors.ink, fontSize: 22, lineHeight: 28 },
    line: {
      height: 1,
      backgroundColor: colors.separator,
      marginHorizontal: 16,
    },
    notice: { color: '#ccc', padding: 16, fontSize: 13 },
  });
