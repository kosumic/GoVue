import { useEffect, useRef, useState } from 'react';
import { ActivityIndicator, Text, useWindowDimensions, View } from 'react-native';
import { NativeCaptionSelection } from '../../modules/govue-core/CaptionSelection';
import { captionSelection, type CaptionRange, type WordSelection } from '../core/vocabulary';
import { useThemeColors } from '../theme';

export function CaptionSelection({ initial, initialHeight, onChange, onPending }: {
  initial: WordSelection;
  initialHeight: number;
  onChange: (selection: WordSelection) => void;
  onPending: (pending: boolean) => void;
}) {
  const colors = useThemeColors();
  const { fontScale } = useWindowDimensions();
  const [plan, setPlan] = useState<CaptionRange | null>(null);
  const [height, setHeight] = useState(initialHeight);
  const [error, setError] = useState('');
  const epoch = useRef(0);
  const request = useRef(0);
  const latestRange = useRef<{ selectionStart: number; selectionEnd: number } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const callbacks = useRef({ onChange, onPending });
  callbacks.current = { onChange, onPending };
  useEffect(() => {
    const version = ++epoch.current;
    callbacks.current.onPending(true);
    void captionSelection(initial).then(range => {
      if (version !== epoch.current) return;
      setPlan(range);
      callbacks.current.onChange({ ...initial, ...range });
      callbacks.current.onPending(false);
    }).catch(failure => { if (version === epoch.current) setError(failure instanceof Error ? failure.message : 'Could not select this caption.'); });
    return () => {
      ++epoch.current;
      if (timer.current) clearTimeout(timer.current);
      timer.current = null;
      latestRange.current = null;
    };
  }, [initial]);
  function select(range: { selectionStart: number; selectionEnd: number }) {
    if (!Number.isInteger(range.selectionStart) || !Number.isInteger(range.selectionEnd) || range.selectionStart < 0 || range.selectionEnd < range.selectionStart) return;
    const version = epoch.current;
    latestRange.current = range;
    callbacks.current.onPending(true);
    if (range.selectionStart === range.selectionEnd) {
      ++epoch.current;
      if (timer.current) clearTimeout(timer.current);
      timer.current = null;
      setError('Select at least one word.');
      return;
    }
    // Coalesce movement without waiting for the finger to stop: the card's
    // selected text follows a continuous drag, with at most one request per tick.
    if (timer.current) return;
    timer.current = setTimeout(() => {
      timer.current = null;
      const offsets = latestRange.current;
      if (!offsets) return;
      const currentRequest = ++request.current;
      void captionSelection(initial, offsets).then(selected => {
        if (version !== epoch.current || currentRequest !== request.current) return;
        setError('');
        callbacks.current.onChange({ ...initial, ...selected });
        if (latestRange.current === offsets) callbacks.current.onPending(false);
      }).catch(failure => {
        if (version === epoch.current && currentRequest === request.current && latestRange.current === offsets)
          setError(failure instanceof Error ? failure.message : 'Choose words in this caption.');
      });
    }, 60);
  }
  return <View>
    {plan ? <NativeCaptionSelection text={initial.source.segments[initial.segmentIndex].text}
      selectionStart={plan.selectionStart} selectionEnd={plan.selectionEnd} fontSize={15 * fontScale} lineHeight={24 * fontScale}
      textColor={colors.ink} selectionColor={colors.action} style={{ height }}
      onSelectionChange={event => select(event.nativeEvent)} onContentSizeChange={event => {
        const value = event.nativeEvent.height;
        if (Number.isFinite(value) && value > 0 && value < 100000) setHeight(Math.ceil(value));
      }} /> : !error && <ActivityIndicator accessibilityLabel="Preparing text selection" color={colors.accent} />}
    {!!error && <Text accessibilityRole="alert" style={{ color: colors.error, fontSize: 13 }}>{error}</Text>}
  </View>;
}
