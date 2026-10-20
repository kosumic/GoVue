import { useEffect, useRef, useState, type ReactNode } from 'react';
import { ActivityIndicator, Alert, Image, Linking, Modal, Pressable, ScrollView, Share, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { ArrowLeft, AtSign, ChevronDown, ChevronRight, CodeXml, Download, ExternalLink, FileDown, FileText, RefreshCw, Scale, Share2, Trash2 } from 'lucide-react-native';
import { coreRequest } from '../../modules/govue-core';
import { version as appVersion } from '../../package.json';
import { checkAppUpdate, DEVELOPER_URL, LATEST_RELEASE_URL, REPOSITORY_URL, WHIP_DOWNLOAD_URL, type AppUpdate, type LicenseNotice } from '../core/app';
import { useHaptic } from '../player/Haptics';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';

function run(action: () => Promise<unknown>) {
  void action().catch(error => Alert.alert('GoVue', error instanceof Error ? error.message : 'Please try again.'));
}
const open = (url: string) => run(() => Linking.openURL(url));
const share = (title: string, message: string) => run(() => Share.share({ title, message }));

function AboutAction({ title, detail, icon, onPress, external, expanded, disabled, busy }: {
  title: string; detail?: string; icon: ReactNode; onPress: () => void;
  external?: boolean; expanded?: boolean; disabled?: boolean; busy?: boolean;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const vibrate = useHaptic();
  return (
    <Pressable accessibilityRole={external ? 'link' : 'button'} accessibilityLabel={title}
      accessibilityState={{ disabled, busy, expanded }} disabled={disabled}
      onPress={() => { vibrate(); onPress(); }}
      style={({ pressed }) => [s.row, pressed && s.pressed, disabled && s.disabled]}>
      <View accessible={false} style={s.icon}>{icon}</View>
      <View style={s.grow}>
        <Text style={s.label}>{title}</Text>
        {!!detail && <Text style={s.detail}>{detail}</Text>}
      </View>
      {busy ? <ActivityIndicator color={colors.action} />
        : external ? <ExternalLink size={18} color={colors.muted} />
          : expanded ? <ChevronDown size={20} color={colors.muted} />
            : <ChevronRight size={20} color={colors.muted} />}
    </Pressable>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  const s = useThemeStyles(makeStyles);
  return <View style={s.section}>
    <Text accessibilityRole="header" style={s.sectionTitle}>{title}</Text>
    {children}
  </View>;
}

export function AboutContent({ onClearCache, onExportDiagnostics }: {
  onClearCache: () => void; onExportDiagnostics: (share: boolean) => Promise<unknown>;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const vibrate = useHaptic();
  const [commit, setCommit] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [update, setUpdate] = useState<AppUpdate | null>(null);
  const [updateError, setUpdateError] = useState('');
  const [licensesOpen, setLicensesOpen] = useState(false);
  const [notice, setNotice] = useState('');
  const request = useRef<AbortController | null>(null);

  useEffect(() => {
    let active = true;
    void coreRequest<{ commit?: string }>({ op: 'info' }).then(info => {
      if (active && info.commit && /^[0-9a-f]{40,64}$/i.test(info.commit)) setCommit(info.commit);
    }).catch(() => {});
    return () => { active = false; request.current?.abort(); request.current = null; };
  }, []);

  async function check() {
    if (request.current) return;
    const controller = new AbortController();
    request.current = controller;
    setChecking(true); setUpdate(null); setUpdateError('');
    const timeout = setTimeout(() => controller.abort(), 15_000);
    try {
      const result = await checkAppUpdate(appVersion, controller.signal);
      if (controller.signal.aborted) throw Error('Update check timed out.');
      if (request.current === controller) setUpdate(result);
    } catch (error) {
      if (request.current === controller) setUpdateError(controller.signal.aborted
        ? 'Update check timed out. Please try again.'
        : error instanceof Error ? error.message : 'Could not check for updates. Please try again.');
    } finally {
      clearTimeout(timeout);
      if (request.current === controller) { request.current = null; setChecking(false); }
    }
  }

  return <>
    <View style={s.brand}>
      {/* Metro resolves bundled images through literal require calls. */}
      {/* eslint-disable-next-line @typescript-eslint/no-require-imports */}
      <Image accessible={false} source={require('../../assets/icon.png')} style={s.appIcon} />
      <Text accessibilityRole="header" style={s.name}>GoVue</Text>
      <Text style={s.tagline}>Watch YouTube. Learn from every line.</Text>
      <Text style={s.version}>Version {appVersion}</Text>
      {commit && <Pressable accessibilityRole="link" accessibilityLabel={`Open commit ${commit.slice(0, 12)}`}
        onPress={() => { vibrate(); open(`${REPOSITORY_URL}/commit/${commit}`); }} style={({ pressed }) => [s.commit, pressed && s.pressed]}>
        <Text style={s.commitText}>Commit {commit.slice(0, 12)}</Text>
        <ExternalLink size={12} color={colors.muted} />
      </Pressable>}
    </View>

    <Section title="Source & community">
      <View style={s.sourceRow}>
        <View style={s.grow}><AboutAction title="GitHub repository" detail="kosumic/GoVue"
          icon={<CodeXml size={23} color={colors.ink} />} external onPress={() => open(REPOSITORY_URL)} /></View>
        <Pressable accessibilityRole="button" accessibilityLabel="Share repository"
          onPress={() => { vibrate(); share('GoVue source code', REPOSITORY_URL); }}
          style={({ pressed }) => [s.share, pressed && s.pressed]}>
          <Share2 size={21} color={colors.ink} />
        </Pressable>
      </View>
      <AboutAction title="Follow the developer" detail="@Kosumi1989 on X"
        icon={<AtSign size={23} color={colors.ink} />} external onPress={() => open(DEVELOPER_URL)} />
    </Section>

    <Section title="Updates & sharing">
      <AboutAction title={checking ? 'Checking for updates…' : 'Check for updates'}
        detail="Check the latest stable release on GitHub" icon={<RefreshCw size={23} color={colors.ink} />}
        disabled={checking} busy={checking} onPress={() => void check()} />
      {!!(updateError || update) && <Text accessibilityLiveRegion="polite" style={s.status}>
        {updateError || (update?.updateAvailable ? `Version ${update.latestVersion} is available.` : 'You’re up to date.')}
      </Text>}
      {update?.updateAvailable && <AboutAction title="View update" detail={`GoVue ${update.latestVersion}`}
        icon={<Download size={23} color={colors.ink} />} external onPress={() => open(LATEST_RELEASE_URL)} />}
      <AboutAction title="Share GoVue" detail="Send someone the latest release"
        icon={<Share2 size={23} color={colors.ink} />}
        onPress={() => share('GoVue', `GoVue — Watch YouTube. Learn from every line.\n${LATEST_RELEASE_URL}`)} />
    </Section>

    <Section title="Check out my other apps">
      <AboutAction title="Whip" detail="Download Whip for Android and iOS"
        icon={<Download size={23} color={colors.ink} />} external onPress={() => open(WHIP_DOWNLOAD_URL)} />
    </Section>

    <Section title="Credits & licenses">
      <AboutAction title="LiTube" detail="Based on HydeYYHH/Litube · GPL-3.0"
        icon={<CodeXml size={23} color={colors.ink} />} external onPress={() => open('https://github.com/HydeYYHH/Litube')} />
      <AboutAction title="Open-source licenses" detail="GoVue’s license and bundled third-party notices"
        icon={<Scale size={23} color={colors.ink} />} onPress={() => setLicensesOpen(true)} />
    </Section>

    <Section title="Troubleshooting">
      <AboutAction title="Clear cache" detail="Remove cached YouTube web resources"
        icon={<Trash2 size={23} color={colors.ink} />} onPress={() => Alert.alert('Clear cache?', 'Remove cached web resources from YouTube.', [
          { text: 'Cancel', style: 'cancel' },
          { text: 'Clear', onPress: () => { onClearCache(); setNotice('Cache cleared.'); } },
        ])} />
      <AboutAction title="Share diagnostics" detail="App, core, and media transport information"
        icon={<FileText size={23} color={colors.ink} />} onPress={() => run(() => onExportDiagnostics(true))} />
      <AboutAction title="Save diagnostics" detail="Save a diagnostic JSON file to your device"
        icon={<FileDown size={23} color={colors.ink} />} onPress={() => run(() => onExportDiagnostics(false))} />
      {!!notice && <Text accessibilityLiveRegion="polite" style={s.status}>{notice}</Text>}
    </Section>
    <LicenseReader visible={licensesOpen} onClose={() => setLicensesOpen(false)} />
  </>;
}

function LicenseReader({ visible, onClose }: { visible: boolean; onClose: () => void }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const insets = useSafeAreaInsets();
  const [notices, setNotices] = useState<LicenseNotice[]>([]);
  const [selected, setSelected] = useState<LicenseNotice | null>(null);
  const [text, setText] = useState('');
  const [error, setError] = useState('');
  const scroll = useRef<ScrollView>(null);

  useEffect(() => {
    if (!visible) return;
    let active = true;
    setError(''); setText('');
    scroll.current?.scrollTo({ y: 0, animated: false });
    if (selected) {
      void coreRequest<{ text: string }>({ op: 'app_license', id: selected.id })
        .then(result => { if (active) setText(result.text); })
        .catch(() => { if (active) setError('Could not load this license. Close it and try again.'); });
    } else {
      void coreRequest<LicenseNotice[]>({ op: 'app_licenses' })
        .then(result => { if (active) setNotices(result); })
        .catch(() => { if (active) setError('Could not load license notices. Close and try again.'); });
    }
    return () => { active = false; };
  }, [selected, visible]);
  const back = () => { if (selected) setSelected(null); else onClose(); };

  return <Modal visible={visible} onRequestClose={back} onDismiss={() => setSelected(null)}>
    <View style={[s.licensePage, { paddingTop: insets.top, paddingBottom: insets.bottom }]}>
      <View style={s.licenseHeading}>
        <Pressable accessibilityRole="button" accessibilityLabel={selected ? 'Back to licenses' : 'Back to About'}
          style={({ pressed }) => [s.back, pressed && s.pressed]} onPress={back}>
          <ArrowLeft size={24} color={colors.ink} />
        </Pressable>
        <Text accessibilityRole="header" style={s.licenseTitle}>{selected?.title || 'Open-source licenses'}</Text>
      </View>
      <ScrollView ref={scroll} contentContainerStyle={s.licenseBody}>
        {error ? <Text accessibilityLiveRegion="polite" style={s.status}>{error}</Text>
          : selected ? text ? <Text selectable style={s.licenseText}>{text}</Text> : <ActivityIndicator color={colors.action} />
            : <>
              <Text style={s.licenseIntro}>GoVue is free software under the GNU General Public License v3.0. These notices are bundled with the app and available offline.</Text>
              {notices.map(notice => <AboutAction key={notice.id} title={notice.title} detail={notice.license}
                icon={<Scale size={23} color={colors.ink} />} onPress={() => setSelected(notice)} />)}
            </>}
      </ScrollView>
    </View>
  </Modal>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  brand: { alignItems: 'center', paddingHorizontal: 24, paddingTop: 28, paddingBottom: 24 },
  appIcon: { width: 80, height: 80, borderRadius: 18, marginBottom: 16 },
  name: { color: colors.ink, fontSize: 28, lineHeight: 36, fontWeight: '600' },
  tagline: { color: colors.muted, fontSize: 14, lineHeight: 21, textAlign: 'center', marginTop: 6 },
  version: { color: colors.muted, fontSize: 12, lineHeight: 18, marginTop: 10 },
  commit: { minHeight: 44, flexDirection: 'row', alignItems: 'center', gap: 6, paddingHorizontal: 12, borderRadius: 8 },
  commitText: { color: colors.muted, fontSize: 12, textDecorationLine: 'underline' },
  section: { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: colors.separator, paddingTop: 20, paddingBottom: 16, marginHorizontal: 16 },
  sectionTitle: { color: colors.muted, fontSize: 13, lineHeight: 20, fontWeight: '600', paddingHorizontal: 4, marginBottom: 8 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 16, minHeight: 72, paddingHorizontal: 4, paddingVertical: 14, borderRadius: 8 },
  pressed: { backgroundColor: colors.accentSoft },
  disabled: { opacity: 0.65 },
  icon: { width: 28, alignItems: 'center' },
  grow: { flex: 1, minWidth: 0 },
  label: { color: colors.ink, fontSize: 15, lineHeight: 22, fontWeight: '500' },
  detail: { color: colors.muted, fontSize: 12, lineHeight: 18, marginTop: 3 },
  sourceRow: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  share: { width: 48, minHeight: 48, alignItems: 'center', justifyContent: 'center', borderLeftWidth: StyleSheet.hairlineWidth, borderLeftColor: colors.separator, borderRadius: 8 },
  status: { color: colors.muted, fontSize: 13, lineHeight: 20, paddingHorizontal: 4, paddingVertical: 10 },
  licensePage: { flex: 1, backgroundColor: colors.background },
  licenseHeading: { flexDirection: 'row', alignItems: 'center', paddingRight: 16 },
  back: { width: 48, minHeight: 48, alignItems: 'center', justifyContent: 'center' },
  licenseTitle: { flex: 1, color: colors.ink, fontSize: 20, lineHeight: 28, fontWeight: '500', paddingVertical: 12 },
  licenseBody: { padding: 16, paddingBottom: 32, width: '100%', maxWidth: 720, alignSelf: 'center' },
  licenseIntro: { color: colors.muted, fontSize: 14, lineHeight: 22, marginBottom: 20 },
  licenseText: { color: colors.ink, fontSize: 13, lineHeight: 21 },
});
