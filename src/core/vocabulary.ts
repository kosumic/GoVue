import { coreRequest } from '../../modules/govue-core';
import type { Transcript } from '../youtube/transcript';

export type WordToken = { text: string; word: boolean; start?: number; end?: number; bold?: boolean; italic?: boolean; underline?: boolean; reading?: string; speakerChange?: boolean };
export type SavedWord = {
  id: string;
  word: string;
  language: string;
  videoId: string;
  title: string;
  start: number;
  context: string;
  savedAt: number;
};

export const tokenizeCaption = (text: string) => coreRequest<WordToken[]>({ op: 'vocabulary_tokenize', text });
export const captionWords = (data: Transcript, segmentIndex: number) =>
  coreRequest<{ text: string; tokens: WordToken[] }>({ op: 'vocabulary_caption', videoId: data.videoId, trackId: data.trackId, segmentIndex });
export const captionDetails = (data: Transcript) =>
  coreRequest<string>({ op: 'vocabulary_details', videoId: data.videoId, trackId: data.trackId });
export const listWords = (query = '') => coreRequest<SavedWord[]>({ op: 'vocabulary_list', query });
export const removeWord = (id: string) => coreRequest<null>({ op: 'vocabulary_remove', id });
export const saveWord = (data: Transcript, segmentIndex: number, tokenIndex: number, word: string, endTokenIndex?: number) =>
  coreRequest<{ entry: SavedWord; saved: boolean }>({ op: 'vocabulary_save', videoId: data.videoId, trackId: data.trackId, segmentIndex, tokenIndex, word, ...(endTokenIndex !== undefined ? { endTokenIndex } : {}) });

export type WordSelection = { source: Transcript; segmentIndex: number; tokenIndex: number; endTokenIndex?: number; word: string };
export type CaptionRange = { tokenIndex: number; endTokenIndex: number; word: string; selectionStart: number; selectionEnd: number };
export const captionSelection = (selection: WordSelection, range?: { selectionStart: number; selectionEnd: number }) =>
  coreRequest<CaptionRange>({ op: 'vocabulary_selection', videoId: selection.source.videoId, trackId: selection.source.trackId, segmentIndex: selection.segmentIndex, tokenIndex: selection.tokenIndex, ...(range ?? {}) });
export type WordTranslationPlan = { input: string; sourceLangCode: string; targetLangCode: string; targetLanguage: string; sameLanguage: boolean };
export const wordAudioTiming = ({ source, segmentIndex, tokenIndex, endTokenIndex, word }: WordSelection) =>
  coreRequest<{ start: number; end: number } | null>({ op: 'vocabulary_word_audio', videoId: source.videoId, trackId: source.trackId, segmentIndex, tokenIndex, endTokenIndex, word });
export const planWordTranslation = ({ source, segmentIndex, tokenIndex, endTokenIndex, word }: WordSelection) =>
  coreRequest<WordTranslationPlan[]>({ op: 'vocabulary_translation', videoId: source.videoId, trackId: source.trackId, segmentIndex, tokenIndex, endTokenIndex, word });
