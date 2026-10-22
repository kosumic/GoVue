import { useEffect, useRef, useState } from 'react';
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, Text, useWindowDimensions, View } from 'react-native';
import { BookOpen, BookmarkPlus, Check, Copy, RotateCw, Sparkles, Square, Volume2, X } from 'lucide-react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { planWordTranslation, saveWord, wordAudioTiming, type WordAudioTiming, type WordSelection, type WordTranslationPlan } from '../core/vocabulary';
import { stopCaptionAudio, useWordAudio } from '../ai/useGameAudio';
import { useWordAssistance, type WordExample, type WordGrammar } from '../ai/useWordAssistance';
import { useAi } from '../ai/useAi';
import { useExampleSpeech } from '../ai/useExampleSpeech';
import { translateWord } from '../native/translation';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { IconButton } from './IconButton';
import { useCopyFeedback } from './useCopyFeedback';
import { WordAiProviderPicker } from './WordAiProviderPicker';
type Translation = { plan: WordTranslationPlan; text: string | null; error: string };

export function WordTranslationSheet({ selection, anchor, pending = false, frame, onClose, onBeforeAudio }: {
  selection: WordSelection;
  anchor?: { x: number; y: number };
  pending?: boolean;
  frame?: { x: number; y: number; width: number; height: number };
  onClose: () => void;
  onBeforeAudio?: () => Promise<void> | void;
}) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const { copied, copyText } = useCopyFeedback();
  const insets = useSafeAreaInsets();
  const window = useWindowDimensions();
  const [cardHeight, setCardHeight] = useState(0);
  const viewportWidth = frame?.width ?? window.width;
  const viewportHeight = frame?.height ?? window.height;
  const width = Math.min(420, viewportWidth - 32);
  const topEdge = frame ? 12 : insets.top + 16;
  const bottomEdge = frame ? viewportHeight - 12 : viewportHeight - insets.bottom - 16;
  const point = anchor && Number.isFinite(anchor.x) && Number.isFinite(anchor.y) ? { x: anchor.x - (frame?.x ?? 0), y: anchor.y - (frame?.y ?? 0) } : undefined;
  const anchorSpace = point ? Math.max(bottomEdge - point.y - 24, point.y - topEdge - 24) : bottomEdge - topEdge;
  const maxHeight = Math.min(viewportHeight * 0.7, bottomEdge - topEdge, Math.max(120, anchorSpace));
  const height = Math.min(cardHeight, maxHeight);
  const left = Math.max(16, Math.min(viewportWidth - width - 16, (point?.x ?? viewportWidth / 2) - width / 2));
  const preferredTop = point ? point.y + 24 + height <= bottomEdge ? point.y + 24 : point.y - height - 24
    : topEdge + (bottomEdge - topEdge - height) / 2;
  const top = Math.max(topEdge, Math.min(bottomEdge - height, preferredTop));
  const [results, setResults] = useState<Translation[] | null>(null);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const [saveStatus, setSaveStatus] = useState('');
  const [saving, setSaving] = useState(false);
  const [audioKind, setAudioKind] = useState<WordAudioTiming['kind'] | null>(null);
  const ai = useAi(true, 'word-assistance');
  const provider = ai.providers.find(p => p.id === ai.activeId);
  const group = ai.groups.find(g => g.id === ai.activeId);
  const providerKey = `${ai.activeId ?? ''}\0${provider?.model ?? group?.name ?? ''}`;
  const aiAvailable = !pending && ai.ready && !ai.busy && (!!group?.connected || !!provider?.enabled && provider.connected && !provider.needsReauth && provider.models.some(m => m.id === provider.model && !m.hidden));
  const example = useWordAssistance<WordExample>(selection, 'example', providerKey, !pending);
  const grammar = useWordAssistance<WordGrammar>(selection, 'grammar', providerKey, !pending);
  const phrase = selection.endTokenIndex !== undefined && selection.endTokenIndex !== selection.tokenIndex;
  const audio = useWordAudio({ videoId: selection.source.videoId, trackId: selection.source.trackId, segmentIndex: selection.segmentIndex, tokenIndex: selection.tokenIndex, endTokenIndex: selection.endTokenIndex, word: selection.word }, onBeforeAudio);
  const speech = useExampleSpeech(selection, async () => { stopCaptionAudio(); await onBeforeAudio?.(); });
  useEffect(() => { speech.stop(); }, [example.result]);
  useEffect(() => { if (pending) { speech.stop(); audio.stop(); } }, [pending]);
  const activeSelection = useRef<WordSelection | null>(selection);
  activeSelection.current = selection;
  useEffect(() => {
    let active = true;
    setResults(null); setError('');
    if (pending) return;
    void (async () => {
      const plans = await planWordTranslation(selection);
      if (!active) return;
      setResults(plans.map(plan => ({ plan, text: null, error: '' })));
      for (const [index, plan] of plans.entries()) {
        if (!active) break;
        let text: string | null = null, error = '';
        try { text = await translateWord(plan, () => active); }
        catch (failure) { error = failure instanceof Error ? failure.message : 'Could not translate this word.'; }
        if (active) setResults(current => current?.map((item, i) => i === index ? { ...item, text, error } : item) ?? null);
      }
    })().catch(failure => { if (active) setError(failure instanceof Error ? failure.message : 'Could not translate this word.'); });
    return () => { active = false; };
  }, [selection, pending, revision]);
  useEffect(() => { setSaveStatus(''); setSaving(false); }, [selection]);
  useEffect(() => {
    let active = true;
    setAudioKind(null);
    void wordAudioTiming(selection).then(timing => { if (active) setAudioKind(timing?.kind ?? null); }).catch(() => {});
    return () => { active = false; };
  }, [selection]);
  useEffect(() => {
    activeSelection.current = selection;
    return () => { activeSelection.current = null; };
  }, [selection]);
  async function save() {
    if (saving || pending) return;
    setSaving(true); setSaveStatus('');
    try {
      const { saved } = await saveWord(selection.source, selection.segmentIndex, selection.tokenIndex, selection.word, selection.endTokenIndex);
      if (activeSelection.current === selection) setSaveStatus(phrase ? saved ? 'Phrase saved' : 'Phrase already saved' : saved ? 'Word saved' : 'Word already saved');
    } catch (failure) {
      if (activeSelection.current === selection) setSaveStatus(failure instanceof Error ? failure.message : 'Could not save word.');
    } finally { if (activeSelection.current === selection) setSaving(false); }
  }

  return <View pointerEvents="box-none" style={styles.overlay}>
      <View onLayout={event => setCardHeight(event.nativeEvent.layout.height)} style={[styles.card, { width, maxHeight, left, top, opacity: cardHeight > 0 ? 1 : 0 }]}>
        <View style={styles.heading}><Text accessibilityRole="header" style={styles.title}>{phrase ? 'Translate phrase' : 'Translate word'}</Text><IconButton icon={X} label="Close word translation" onPress={onClose} /></View>
        <ScrollView style={styles.scroll} contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
          <View style={styles.wordHeading}>
            <Text selectable style={styles.word}>{selection.word}</Text>
            <View style={styles.wordActions}>
              <Pressable accessibilityRole="button" accessibilityLabel={phrase ? copied ? 'Phrase copied' : 'Copy phrase' : copied ? 'Word copied' : 'Copy word'} disabled={pending} accessibilityState={{ disabled: pending }} accessibilityLiveRegion="polite" onPress={() => copyText(selection.word)} style={({ pressed }) => [styles.actionButton, (pending || pressed) && styles.disabled]}>
                {copied ? <Check size={24} color={colors.accent} /> : <Copy size={24} color={colors.ink} />}
              </Pressable>
              {audioKind && !pending && <Pressable accessibilityRole="button" accessibilityLabel={audio.phase === 'idle' ? audioKind === 'caption' ? 'Play caption' : phrase ? 'Play phrase' : 'Play word' : audioKind === 'caption' ? 'Stop caption audio' : 'Stop word audio'} accessibilityHint={audioKind === 'caption' ? 'Plays the full caption containing this selection.' : undefined} accessibilityState={{ busy: audio.phase === 'loading' }} onPress={() => {
                speech.stop();
                if (audio.phase === 'idle') void audio.play('word').catch(() => {}); else audio.stop();
              }} style={({ pressed }) => [styles.actionButton, pressed && styles.disabled]}>
                {audio.phase === 'loading' ? <ActivityIndicator color={colors.accent} /> : audio.phase === 'playing' ? <Square size={24} color={colors.ink} /> : <Volume2 size={24} color={colors.ink} />}
              </Pressable>}
              <Pressable accessibilityRole="button" accessibilityLabel={phrase ? 'Save phrase' : 'Save word'} accessibilityState={{ disabled: saving || pending, busy: saving }} disabled={saving || pending} onPress={() => { void save(); }} style={({ pressed }) => [styles.actionButton, (saving || pending || pressed) && styles.disabled]}>
                {saving ? <ActivityIndicator color={colors.accent} /> : <BookmarkPlus size={24} color={colors.ink} />}
              </Pressable>
            </View>
          </View>
          <Text style={styles.help}>Drag either handle to change the selection.</Text>
          {pending ? <ActivityIndicator accessibilityLabel="Updating selection" color={colors.accent} /> : error ? <Text accessibilityRole="alert" style={styles.error}>{error}</Text> : results ? results.map(result =>
            <View key={result.plan.targetLangCode} accessibilityLiveRegion="polite" style={styles.result}>
              <Text style={styles.label}>{result.plan.targetLanguage}</Text>
              {result.error ? <Text accessibilityRole="alert" style={styles.error}>{result.error}</Text> : result.text !== null ?
                <><Text selectable style={styles.translated}>{result.text}</Text>{result.plan.sameLanguage && <Text style={styles.help}>Already in {result.plan.targetLanguage}.</Text>}</> :
                <View style={styles.translation}><ActivityIndicator color={colors.accent} /><Text style={styles.help}>Translating…</Text></View>}
            </View>) : <ActivityIndicator accessibilityLabel="Preparing word translations" color={colors.accent} />}
          {results?.some(result => result.text === null && !result.error) && <Text style={styles.help}>The first translation may download language models.</Text>}
          {(!!error || results?.some(result => !!result.error)) && <IconButton icon={RotateCw} label="Retry word translation" onPress={() => setRevision(value => value + 1)} />}
          {!!audio.error && <Text accessibilityRole="alert" style={styles.error}>{audio.error}</Text>}
          {!!saveStatus && <Text accessibilityLiveRegion="polite" style={styles.help}>{saveStatus}</Text>}
          <Pressable accessibilityRole="button" accessibilityLabel="Generate example" accessibilityState={{ disabled: example.busy || !aiAvailable }} disabled={example.busy || !aiAvailable} onPress={() => { void example.generate(); }} style={[styles.save, !aiAvailable && styles.disabled]}>
            {example.busy ? <ActivityIndicator color={colors.accent} /> : <Sparkles size={19} color={colors.ink} />}<Text style={styles.saveText}>{example.busy ? 'Generating example…' : 'Generate example'}</Text>
          </Pressable>
          {!!example.error && <Text accessibilityRole="alert" style={styles.error}>{example.error}</Text>}
          {example.result && <View accessibilityLiveRegion="polite" style={styles.result}>
            <View style={styles.exampleHeading}><Text style={styles.label}>Example · {example.result.sourceLanguage}</Text>
              <IconButton icon={speech.active === 'example' ? Square : Volume2} label={speech.active === 'example' ? 'Stop example speech' : `Speak example in ${example.result.sourceLanguage}`} onPress={() => {
                if (speech.active === 'example') speech.stop(); else void speech.speak('example', example.result!.sentence, example.result!.sourceLangCode);
              }} />
            </View>
            <Text selectable style={styles.answer}>{example.result.sentence}</Text>
            {example.result.translations.map(translation => <View key={translation.languageCode} style={styles.result}>
              <View style={styles.exampleHeading}><Text style={styles.label}>{translation.language}</Text>
                <IconButton icon={speech.active === translation.languageCode ? Square : Volume2} label={speech.active === translation.languageCode ? `Stop ${translation.language} speech` : `Speak example in ${translation.language}`} onPress={() => {
                  if (speech.active === translation.languageCode) speech.stop(); else void speech.speak(translation.languageCode, translation.text, translation.languageCode);
                }} />
              </View><Text selectable style={styles.answer}>{translation.text}</Text>
            </View>)}
          </View>}
          {!!speech.error && <Text accessibilityRole="alert" style={styles.error}>{speech.error}</Text>}
          <Pressable accessibilityRole="button" accessibilityLabel="Explain grammar" accessibilityState={{ disabled: grammar.busy || !aiAvailable }} disabled={grammar.busy || !aiAvailable} onPress={() => { void grammar.generate(); }} style={[styles.save, !aiAvailable && styles.disabled]}>
            {grammar.busy ? <ActivityIndicator color={colors.accent} /> : <BookOpen size={19} color={colors.ink} />}<Text style={styles.saveText}>{grammar.busy ? 'Explaining grammar…' : 'Explain grammar'}</Text>
          </Pressable>
          {!!grammar.error && <Text accessibilityRole="alert" style={styles.error}>{grammar.error}</Text>}
          {grammar.result && <View accessibilityLiveRegion="polite" style={styles.result}>
            <Text style={styles.label}>Grammar · {grammar.result.language}</Text>
            <Text selectable style={styles.saveText}>Base form: {grammar.result.baseForm}</Text>
            <Text selectable style={styles.answer}>{grammar.result.explanation}</Text>
          </View>}
          <WordAiProviderPicker ai={ai} />
          {!pending && !aiAvailable && <Text style={styles.help}>{ai.error || (!ai.ready ? 'Loading AI providers…' : 'Choose a connected provider and model to generate examples and grammar explanations.')}</Text>}
        </ScrollView>
      </View>
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  overlay: { ...StyleSheet.absoluteFill, zIndex: 10 },
  card: { position: 'absolute', backgroundColor: colors.dialog, borderRadius: 20, borderWidth: StyleSheet.hairlineWidth, borderColor: colors.line, paddingBottom: 12, elevation: 12, boxShadow: '0 8px 32px #0008', overflow: 'hidden' },
  scroll: { flexGrow: 0, flexShrink: 1 },
  heading: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingLeft: 20, paddingRight: 8, paddingTop: 8 },
  title: { color: colors.ink, fontSize: 19, fontWeight: '600' },
  content: { paddingHorizontal: 20, paddingBottom: 8, gap: 14 },
  wordHeading: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 8 },
  word: { flexShrink: 1, color: colors.ink, fontSize: 28, fontWeight: '600' },
  wordActions: { flexDirection: 'row', flexShrink: 0 },
  actionButton: { width: 48, height: 48, borderRadius: 24, alignItems: 'center', justifyContent: 'center' },
  label: { color: colors.muted, fontSize: 12 },
  translation: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 10 },
  result: { gap: 8, paddingVertical: 8, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  translated: { color: colors.ink, fontSize: 24, lineHeight: 34 },
  answer: { color: colors.ink, fontSize: 15, lineHeight: 23 },
  exampleHeading: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
  help: { flexShrink: 1, color: colors.muted, fontSize: 13, lineHeight: 20 },
  error: { flexShrink: 1, color: colors.error, fontSize: 14, lineHeight: 21 },
  save: { minHeight: 48, borderRadius: 10, backgroundColor: colors.accentSoft, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8 },
  saveText: { color: colors.ink, fontSize: 15, fontWeight: '600' },
  disabled: { opacity: 0.45 },
});
