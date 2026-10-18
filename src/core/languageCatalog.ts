import { coreRequest } from '../../modules/govue-core';

export type LanguageOption = { code: string; name: string; nativeName: string };
export type LanguageChoices = {
  selected: LanguageOption | null;
  sections: { title: string; data: LanguageOption[] }[];
};

export const languageChoices = (query: string, excluded: readonly string[], selected: string) =>
  coreRequest<LanguageChoices>({ op: 'language_choices', query, excluded, selected });
