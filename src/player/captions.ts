export type CaptionFrame = { text: string; sourceText: string; segmentIndex: number };
export type CaptionLayout = { sourceText: string; segmentIndex: number; lines: string[] };
export const emptyCaptionFrame: CaptionFrame = { text: '', sourceText: '', segmentIndex: -1 };
