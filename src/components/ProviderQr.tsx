import { useEffect, useMemo, useRef, useState } from 'react';
import { ActivityIndicator, AppState, Linking, Modal, Pressable, ScrollView, StyleSheet, Switch, Text, View, useWindowDimensions } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { CameraView, scanFromURLAsync, useCameraPermissions, type BarcodeScanningResult } from 'expo-camera';
import * as ImagePicker from 'expo-image-picker';
import * as FileSystem from 'expo-file-system/legacy';
import * as Sharing from 'expo-sharing';
import { Flashlight, Image, ScanLine, Share2, X } from 'lucide-react-native';
import Svg, { Defs, G, LinearGradient, Path, Rect, Stop, Text as SvgText } from 'react-native-svg';
import { aiRequest } from '../../modules/govue-core';
import type { AiController, Provider } from '../ai/useAi';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { qrArtwork } from './providerQrArtwork';

type Preview = { label: string; kind: string; baseUrl: string; protocol: string; oauth: boolean; model: string; modelCount: number };
type Qr = { payload: string; modules: string[]; provider: Preview };
const QR_SETTINGS = { barcodeTypes: ['qr' as const] };

function Button({ title, onPress, disabled = false }: { title: string; onPress: () => void; disabled?: boolean }) {
  const s = useThemeStyles(makeStyles);
  return <Pressable accessibilityRole="button" disabled={disabled} onPress={onPress} style={({ pressed }) => [s.button, (disabled || pressed) && s.dim]}><Text style={s.buttonText}>{title}</Text></Pressable>;
}

export function ProviderQr({ providerId, ai, onClose, onImported }: { providerId?: string; ai: AiController; onClose: () => void; onImported: (p: Provider) => void }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const { width } = useWindowDimensions();
  const [permission, requestPermission, getPermission] = useCameraPermissions();
  const [foreground, setForeground] = useState(AppState.currentState === 'active');
  const [qr, setQr] = useState<Qr | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [payload, setPayload] = useState('');
  const [selectedOnly, setSelectedOnly] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [torch, setTorch] = useState(false);
  const handled = useRef(false);
  const mounted = useRef(true);
  const active = useRef(foreground);
  const svg = useRef<Svg>(null);
  const artwork = useMemo(() => qr ? qrArtwork(qr.modules) : null, [qr]);
  const sender = ai.providers.find(p => p.id === providerId);
  const scanning = !providerId && !preview;

  useEffect(() => {
    mounted.current = true;
    const subscription = AppState.addEventListener('change', state => {
      active.current = state === 'active';
      setForeground(active.current);
      if (active.current && !providerId) void getPermission().catch(() => {
        if (mounted.current) setError('Could not check camera access. Try opening Settings.');
      });
    });
    return () => { mounted.current = false; active.current = false; subscription.remove(); };
  }, [providerId, getPermission]);

  useEffect(() => {
    if (providerId) return;
    void requestPermission().catch(() => {
      if (mounted.current) setError('Could not request camera access. Try opening Settings.');
    });
  }, [providerId, requestPermission]);

  useEffect(() => {
    if (!providerId) return;
    let current = true;
    setQr(null); setError(''); setBusy(true);
    void aiRequest<Qr>({ op: 'ai_provider_qr', id: providerId, selectedModelOnly: selectedOnly })
      .then(result => { if (current) setQr(result); })
      .catch(e => { if (current) setError(e.message); })
      .finally(() => { if (current) setBusy(false); });
    return () => { current = false; };
  }, [providerId, selectedOnly]);

  async function read(data: string) {
    setError(''); setBusy(true);
    try {
      const result = await aiRequest<Preview>({ op: 'ai_preview_provider_qr', payload: data });
      if (mounted.current) { setPreview(result); setPayload(data); }
    } catch (e) {
      if (mounted.current) setError((e as Error).message);
    } finally { if (mounted.current) setBusy(false); }
  }
  function scan({ data }: BarcodeScanningResult) {
    if (handled.current || !active.current) return;
    handled.current = true;
    void read(data);
  }
  function retry() { handled.current = false; setPreview(null); setPayload(''); setError(''); }
  async function fromImage() {
    if (busy) return;
    handled.current = true; setBusy(true); setError('');
    try {
      const image = await ImagePicker.launchImageLibraryAsync({ mediaTypes: ['images'], quality: 1 });
      if (!mounted.current) return;
      if (image.canceled) { handled.current = false; return; }
      const codes = await scanFromURLAsync(image.assets[0].uri, ['qr']);
      if (!mounted.current) return;
      if (!codes.length) throw new Error('No QR code found. Choose an image with the whole code visible.');
      // Validate every candidate in Rust; an unrelated code must not hide a valid one.
      for (const code of codes) {
        try {
          const result = await aiRequest<Preview>({ op: 'ai_preview_provider_qr', payload: code.data });
          if (mounted.current) { setPreview(result); setPayload(code.data); }
          return;
        } catch { /* Try the next QR in the image. */ }
      }
      throw new Error('This image does not contain a supported GoVue provider QR code.');
    } catch (e) { if (mounted.current) setError((e as Error).message); }
    finally { if (mounted.current) setBusy(false); }
  }
  async function share() {
    if (!svg.current || busy) return;
    setBusy(true); setError('');
    try {
      if (!await Sharing.isAvailableAsync()) throw new Error('Image sharing is unavailable on this device. Scan the code with another phone.');
      const data = await new Promise<string>((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error('Could not create the QR image. Please try again.')), 10000);
        svg.current!.toDataURL(value => { clearTimeout(timeout); resolve(value); }, { width: 1200, height: 1320 });
      });
      // Keep one credential-free image in cache: receivers may read the URI after
      // the share sheet closes. A later share replaces it.
      const path = `${FileSystem.cacheDirectory}govue-provider-qr.png`;
      await FileSystem.writeAsStringAsync(path, data, { encoding: FileSystem.EncodingType.Base64 });
      await Sharing.shareAsync(path, { mimeType: 'image/png', UTI: 'public.png', dialogTitle: 'Share provider QR code' });
    } catch (e) { if (mounted.current) setError((e as Error).message); }
    finally {
      if (mounted.current) setBusy(false);
    }
  }
  async function importProvider() {
    if (busy || !payload) return;
    setBusy(true); setError('');
    const next = await ai.mutate({ op: 'ai_import_provider_qr', payload });
    if (mounted.current) {
      setBusy(false);
      const imported = next?.providers.at(-1);
      if (imported) onImported(imported);
    }
  }
  const details = qr?.provider || preview;
  const extent = artwork?.extent || 1;
  return <Modal visible animationType="fade" onRequestClose={() => { if (!busy) onClose(); }}>
    <SafeAreaView style={s.page}>
      <View style={s.header}><Text style={s.title}>{providerId ? 'Share provider' : preview ? 'Import provider' : 'Scan provider'}</Text><Pressable accessibilityRole="button" accessibilityLabel="Close provider QR" onPress={onClose} disabled={busy} style={s.close}><X color={colors.ink} size={24} /></Pressable></View>
      <ScrollView contentContainerStyle={s.content}>
        {providerId ? <>
          <Text style={s.name}>{sender?.label}</Text>
          <Text style={s.help}>Scan with GoVue on another device.</Text>
          <View style={s.qrPanel}>
            {qr && artwork ? <Svg ref={svg} width={Math.min(width - 72, 360)} height={Math.min(width - 72, 360) * 1.1} viewBox={`0 0 ${extent} ${extent * 1.1}`} accessibilityLabel={`QR code for ${qr.provider.label}`}>
              <Defs><LinearGradient id="providerInk" x1="0" y1="0" x2="1" y2="1"><Stop offset="0" stopColor="#142C42" /><Stop offset="1" stopColor="#176568" /></LinearGradient></Defs>
              <Rect width={extent} height={extent * 1.1} rx={3} fill="#FFFFFF" />
              <G fill="url(#providerInk)"><Path d={artwork.path} />{artwork.eyes.map(([x, y]) => <G key={`${x}:${y}`}><Rect x={x} y={y} width={7} height={7} rx={1.8} /><Rect x={x + 1} y={y + 1} width={5} height={5} rx={1.1} fill="#FFFFFF" /><Rect x={x + 2} y={y + 2} width={3} height={3} rx={0.7} /></G>)}</G>
              <SvgText x={extent / 2} y={extent * 1.065} textAnchor="middle" fill="#142C42" fontSize={extent * 0.048} fontWeight="700" letterSpacing={extent * 0.003}>GoVue</SvgText>
            </Svg> : <View style={s.qrPlaceholder}>{busy ? <ActivityIndicator color="#176568" /> : <ScanLine size={44} color="#176568" />}</View>}
          </View>
          {sender && sender.models.length > 1 && <View style={s.toggle}><Text style={s.label}>Selected model only</Text><Switch accessibilityLabel="Share selected model only" value={selectedOnly} disabled={busy} onValueChange={setSelectedOnly} /></View>}
          {qr && <View style={s.shareButton}><Share2 size={18} color={colors.ink} /><Button title="Share QR image" disabled={busy} onPress={() => { void share(); }} /></View>}
        </> : scanning ? <>
          <View style={s.camera}>
            {permission?.granted && foreground && !error && !busy ? <CameraView style={StyleSheet.absoluteFill} facing="back" barcodeScannerSettings={QR_SETTINGS} enableTorch={torch} onBarcodeScanned={scan} onMountError={() => setError('Could not start the camera. Close the scanner and try again.')} /> : !permission ? <ActivityIndicator color="#FFFFFF" /> : <ScanLine color="#FFFFFF" size={48} />}
            <View pointerEvents="none" style={s.scanFrame} />
          </View>
          <Text style={s.help}>Point your camera at a GoVue provider QR code.</Text>
          {!!permission && !permission.granted && <Button title={permission.canAskAgain ? 'Allow camera' : 'Open Settings'} onPress={() => { void (permission.canAskAgain ? requestPermission() : Linking.openSettings()).catch(() => setError('Could not open camera permissions.')); }} />}
          <View style={s.tools}>
            <Pressable accessibilityRole="button" accessibilityLabel="Choose provider QR image" disabled={busy} onPress={() => { void fromImage(); }} style={s.tool}><Image size={21} color={colors.ink} /><Text style={s.label}>Choose image</Text></Pressable>
            {permission?.granted && <Pressable accessibilityRole="switch" accessibilityLabel="Scanner flashlight" accessibilityState={{ checked: torch }} onPress={() => setTorch(value => !value)} style={s.tool}><Flashlight size={21} color={torch ? colors.action : colors.ink} /><Text style={s.label}>Flashlight</Text></Pressable>}
          </View>
          {!!error && <Button title="Scan again" disabled={busy} onPress={retry} />}
        </> : null}
        {details && <View style={s.details}>
          {!providerId && <Text style={s.name}>{details.label}</Text>}
          <Text selectable style={s.endpoint}>{details.baseUrl}</Text>
          <Text style={s.help}>{details.protocol} · {details.modelCount} {details.modelCount === 1 ? 'model' : 'models'}</Text>
          {!!details.model && <Text style={s.label}>{details.model}</Text>}
          <Text style={s.help}>API keys, account tokens, custom headers and extra parameters are excluded.{details.oauth ? ' Sign in on the receiving device.' : ' Add your API key on the receiving device if required.'}</Text>
        </View>}
        {!!preview && <><Button title="Import provider" disabled={busy} onPress={() => { void importProvider(); }} /><Button title="Scan another code" disabled={busy} onPress={retry} /></>}
        {busy && <ActivityIndicator color={colors.action} />}
        {!!(error || ai.error) && <Text accessibilityRole="alert" style={s.error}>{error || ai.error}</Text>}
      </ScrollView>
    </SafeAreaView>
  </Modal>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  page: { flex: 1, backgroundColor: colors.background },
  header: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingHorizontal: 22, minHeight: 60 },
  title: { fontSize: 18, fontWeight: '600', color: colors.ink }, close: { padding: 12, minHeight: 48, minWidth: 48, alignItems: 'center', justifyContent: 'center' },
  content: { alignItems: 'center', padding: 24, paddingBottom: 48, gap: 18, width: '100%', maxWidth: 520, alignSelf: 'center' },
  name: { color: colors.ink, fontSize: 25, fontWeight: '700', textAlign: 'center' },
  help: { color: colors.muted, fontSize: 13, lineHeight: 20, textAlign: 'center' },
  label: { color: colors.ink, fontSize: 14, lineHeight: 21 },
  qrPanel: { backgroundColor: '#FFFFFF', padding: 12, borderRadius: 28, overflow: 'hidden', alignItems: 'center', justifyContent: 'center' },
  qrPlaceholder: { width: 264, height: 290, alignItems: 'center', justifyContent: 'center' },
  toggle: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', width: '100%', minHeight: 48 },
  shareButton: { flexDirection: 'row', gap: 10, alignItems: 'center' },
  button: { paddingHorizontal: 24, paddingVertical: 14, minHeight: 48, borderRadius: 16, backgroundColor: colors.accentSoft, alignItems: 'center' },
  buttonText: { color: colors.ink, fontSize: 15, fontWeight: '600' }, dim: { opacity: 0.45 },
  details: { alignItems: 'center', gap: 10, width: '100%', paddingTop: 6 },
  endpoint: { color: colors.ink, fontSize: 14, lineHeight: 21, textAlign: 'center' },
  camera: { width: '100%', aspectRatio: 1, backgroundColor: '#142C42', borderRadius: 28, overflow: 'hidden', alignItems: 'center', justifyContent: 'center' },
  scanFrame: { position: 'absolute', width: '76%', height: '76%', borderColor: '#FFFFFF', borderWidth: 2, borderRadius: 24 },
  tools: { flexDirection: 'row', gap: 24, alignItems: 'center', justifyContent: 'center' },
  tool: { minHeight: 48, alignItems: 'center', justifyContent: 'center', gap: 6, padding: 8 },
  error: { color: colors.error, fontSize: 13, lineHeight: 20, textAlign: 'center' },
});
