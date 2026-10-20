import { requireNativeViewManager } from 'expo-modules-core';
import type { NativeSyntheticEvent, ViewProps } from 'react-native';

export type CaptionSelectionProps = ViewProps & {
  text: string;
  selectionStart: number;
  selectionEnd: number;
  fontSize: number;
  lineHeight: number;
  textColor: string;
  selectionColor: string;
  onSelectionChange: (event: NativeSyntheticEvent<{ selectionStart: number; selectionEnd: number }>) => void;
  onContentSizeChange: (event: NativeSyntheticEvent<{ height: number }>) => void;
};

export const NativeCaptionSelection = requireNativeViewManager<CaptionSelectionProps>('GoVueCore');
