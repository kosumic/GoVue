import { useEffect, useRef, useState } from 'react';
import Clipboard from '@react-native-clipboard/clipboard';
import { useHaptic } from '../player/Haptics';

export function useCopyFeedback() {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const haptic = useHaptic();
  useEffect(() => () => { if (timer.current !== null) clearTimeout(timer.current); }, []);
  function copyText(text: string) {
    Clipboard.setString(text);
    haptic();
    if (timer.current !== null) clearTimeout(timer.current);
    setCopied(true);
    timer.current = setTimeout(() => { timer.current = null; setCopied(false); }, 1500);
  }
  return { copied, copyText };
}
