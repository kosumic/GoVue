import { coreRequest } from '../../modules/lingoplay-core';
import type { Transcript } from '../youtube/transcript';

export type WordToken = { text: string; word: boolean };
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
export const listWords = (query = '') => coreRequest<SavedWord[]>({ op: 'vocabulary_list', query });
export const removeWord = (id: string) => coreRequest<null>({ op: 'vocabulary_remove', id });
export const saveWord = (data: Transcript, segmentIndex: number, tokenIndex: number, word: string) =>
  coreRequest<{ entry: SavedWord; saved: boolean }>({ op: 'vocabulary_save', videoId: data.videoId, trackId: data.trackId, segmentIndex, tokenIndex, word });
