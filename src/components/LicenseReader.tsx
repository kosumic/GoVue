import { useEffect, useState } from 'react';
import { ActivityIndicator, Alert, Linking, Modal, Platform, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { ArrowLeft, ChevronDown, ChevronUp, ExternalLink } from 'lucide-react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { coreRequest } from '../../modules/govue-core';
import type { LicenseNotice } from '../core/app';
import { useHaptic } from '../player/Haptics';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';

const groups = [
  { category: 'featured', title: 'Project & credits' },
  { category: 'npm', title: 'npm dependencies' },
  { category: 'cargo', title: 'Rust dependencies' },
] as const;

function LicenseEntry({ notice }: { notice: LicenseNotice }) {
  const s = useThemeStyles(makeStyles);
  const colors = useThemeColors();
  const vibrate = useHaptic();
  const [expanded, setExpanded] = useState(false);
  const [text, setText] = useState('');
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!expanded || text) return;
    let active = true;
    setError('');
    void coreRequest<{ text: string }>({ op: 'app_license', id: notice.id })
      .then(result => { if (active) setText(result.text); })
      .catch(() => { if (active) setError('Could not load this license. Please try again.'); });
    return () => { active = false; };
  }, [expanded, notice.id, text, attempt]);

  return <View style={s.entry}>
    <Text accessibilityRole="header" style={s.project}>{notice.title}</Text>
    <Text style={s.license}>{notice.license}</Text>
    <Text style={s.attribution}>{notice.attribution}</Text>
    <Pressable accessibilityRole="link" accessibilityLabel={`View source for ${notice.title}`}
      onPress={() => {
        vibrate();
        void Linking.openURL(notice.sourceUrl).catch(() => Alert.alert('GoVue', 'Could not open the source repository.'));
      }} style={({ pressed }) => [s.action, pressed && s.pressed]}>
      <Text style={s.link}>View source</Text><ExternalLink size={15} color={colors.action} />
    </Pressable>
    <Pressable accessibilityRole="button" accessibilityLabel={`${expanded ? 'Hide' : 'View'} license for ${notice.title}`}
      accessibilityState={{ expanded }} onPress={() => { vibrate(); setExpanded(!expanded); }}
      style={({ pressed }) => [s.action, pressed && s.pressed]}>
      <Text style={s.actionText}>{expanded ? 'Hide license' : 'View license'}</Text>
      {expanded ? <ChevronUp size={19} color={colors.muted} /> : <ChevronDown size={19} color={colors.muted} />}
    </Pressable>
    {expanded && <View style={s.textBody}>
      {text ? <Text selectable style={s.text}>{text}</Text>
        : error ? <>
          <Text accessibilityLiveRegion="polite" style={s.attribution}>{error}</Text>
          <Pressable accessibilityRole="button" accessibilityLabel={`Retry license for ${notice.title}`}
            onPress={() => { vibrate(); setAttempt(attempt + 1); }} style={s.action}>
            <Text style={s.link}>Try again</Text>
          </Pressable>
        </> : <ActivityIndicator accessibilityLabel={`Loading license for ${notice.title}`} color={colors.action} />}
    </View>}
  </View>;
}

export function LicenseReader({ onClose }: { onClose: () => void }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const insets = useSafeAreaInsets();
  const vibrate = useHaptic();
  const [notices, setNotices] = useState<LicenseNotice[] | null>(null);
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let active = true;
    setError('');
    void coreRequest<LicenseNotice[]>({ op: 'app_licenses' })
      .then(result => { if (active) setNotices(result); })
      .catch(() => { if (active) setError('Could not load license notices. Please try again.'); });
    return () => { active = false; };
  }, [attempt]);

  return <Modal visible onRequestClose={onClose}>
    <View style={[s.page, { paddingTop: insets.top, paddingBottom: insets.bottom }]}>
      <View style={s.heading}>
        <Pressable accessibilityRole="button" accessibilityLabel="Back to About"
          style={({ pressed }) => [s.back, pressed && s.pressed]} onPress={() => { vibrate(); onClose(); }}>
          <ArrowLeft size={24} color={colors.ink} />
        </Pressable>
        <View style={s.grow}>
          <Text accessibilityRole="header" style={s.title}>Open-source licenses</Text>
          {notices && <Text style={s.count}>{notices.length} notices</Text>}
        </View>
      </View>
      <ScrollView contentContainerStyle={s.body}>
        <Text style={s.intro}>GoVue is free software under the GNU General Public License v3.0. These project credits and direct dependency notices are bundled with the app and available offline.</Text>
        {error ? <>
          <Text accessibilityLiveRegion="polite" style={s.attribution}>{error}</Text>
          <Pressable accessibilityRole="button" accessibilityLabel="Retry loading license notices"
            onPress={() => { vibrate(); setAttempt(attempt + 1); }} style={s.action}>
            <Text style={s.link}>Try again</Text>
          </Pressable>
        </> : !notices ? <ActivityIndicator accessibilityLabel="Loading license notices" color={colors.action} />
          : groups.map(group => <View key={group.category}>
            <Text accessibilityRole="header" style={s.section}>{group.title}</Text>
            {notices.filter(notice => notice.category === group.category).map(notice => <LicenseEntry key={notice.id} notice={notice} />)}
          </View>)}
      </ScrollView>
    </View>
  </Modal>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  page: { flex: 1, backgroundColor: colors.background },
  heading: { flexDirection: 'row', alignItems: 'center', paddingRight: 16, paddingVertical: 10 },
  back: { width: 48, minHeight: 48, alignItems: 'center', justifyContent: 'center', borderRadius: 8 },
  grow: { flex: 1, minWidth: 0 },
  title: { color: colors.ink, fontSize: 20, lineHeight: 28, fontWeight: '500' },
  count: { color: colors.muted, fontSize: 12, lineHeight: 18, marginTop: 2 },
  body: { padding: 20, paddingBottom: 40, width: '100%', maxWidth: 720, alignSelf: 'center' },
  intro: { color: colors.muted, fontSize: 14, lineHeight: 22, marginBottom: 8 },
  section: { color: colors.muted, fontSize: 13, lineHeight: 20, fontWeight: '600', paddingTop: 24, paddingBottom: 4 },
  entry: { borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: colors.separator, paddingTop: 20, paddingBottom: 12 },
  project: { color: colors.ink, fontSize: 16, lineHeight: 23, fontWeight: '600' },
  license: { color: colors.muted, fontSize: 12, lineHeight: 18, marginTop: 3 },
  attribution: { color: colors.muted, fontSize: 14, lineHeight: 22, marginTop: 10 },
  action: { minHeight: 48, flexDirection: 'row', alignItems: 'center', gap: 8, alignSelf: 'flex-start', paddingHorizontal: 8, marginLeft: -8, borderRadius: 8 },
  actionText: { color: colors.ink, fontSize: 14, lineHeight: 21, fontWeight: '500' },
  link: { color: colors.action, fontSize: 14, lineHeight: 21, textDecorationLine: 'underline' },
  pressed: { backgroundColor: colors.accentSoft },
  textBody: { paddingVertical: 16 },
  text: { color: colors.ink, fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace', fontSize: 12, lineHeight: 20 },
});
