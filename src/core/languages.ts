import { coreRequest } from '../../modules/lingoplay-core';

export type CefrLevel = 'A1' | 'A2' | 'B1' | 'B2' | 'C1' | 'C2';
export type LanguageLevel = CefrLevel | 'native';
export type LanguageEntry = { language: string; level: LanguageLevel };
export type LanguageSettings = { known: LanguageEntry[]; learning: LanguageEntry[] };

// Plain-language summaries of the Council of Europe's CEFR global scale.
export const languageLevels: { value: LanguageLevel; title: string; explanation: string }[] = [
  { value: 'A1', title: 'Beginner', explanation: 'Use basic phrases, introduce yourself, and ask simple questions with help.' },
  { value: 'A2', title: 'Elementary', explanation: 'Handle routine exchanges about shopping, family, work, and familiar needs.' },
  { value: 'B1', title: 'Intermediate', explanation: 'Follow familiar topics, manage most travel situations, and explain experiences and plans.' },
  { value: 'B2', title: 'Upper intermediate', explanation: 'Understand complex topics, converse comfortably, and explain viewpoints in detail.' },
  { value: 'C1', title: 'Advanced', explanation: 'Understand implied meaning and use the language fluently for work, study, and social life.' },
  { value: 'C2', title: 'Proficient', explanation: 'Understand almost everything you hear or read and express subtle meanings precisely.' },
  { value: 'native', title: 'Native', explanation: 'A language you grew up speaking. This is separate from the CEFR scale.' },
];

export const languageSettings = (settings?: LanguageSettings) =>
  coreRequest<LanguageSettings>({ op: 'language_settings', settings });
