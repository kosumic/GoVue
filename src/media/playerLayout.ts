export type PlayerLayout = {
  left: number;
  top: number;
  width: number;
  height: number;
  viewportWidth: number;
  viewportHeight: number;
  visible: boolean;
};

// Layout messages share the same untrusted page boundary as media messages.
export function acceptPlayerLayout(value: unknown): PlayerLayout | null {
  if (!value || typeof value !== 'object') return null;
  const layout = value as PlayerLayout;
  const numbers = [layout.left, layout.top, layout.width, layout.height, layout.viewportWidth, layout.viewportHeight];
  if (!numbers.every(n => typeof n === 'number' && Number.isFinite(n)) || typeof layout.visible !== 'boolean') return null;
  if (layout.viewportWidth <= 0 || layout.viewportWidth > 10000 || layout.viewportHeight <= 0 || layout.viewportHeight > 10000 ||
      layout.width < 0 || layout.width > layout.viewportWidth * 4 || layout.height < 0 || layout.height > layout.viewportHeight * 4 ||
      Math.abs(layout.left) > layout.viewportWidth * 4 || Math.abs(layout.top) > layout.viewportHeight * 4) return null;
  return { left: layout.left, top: layout.top, width: layout.width, height: layout.height,
    viewportWidth: layout.viewportWidth, viewportHeight: layout.viewportHeight, visible: layout.visible };
}

export function embeddedPlayerFrame(layout: PlayerLayout | null, width: number, height: number) {
  if (!layout) return { left: 0, top: 0, width, height: Math.min(height, width * 9 / 16) };
  const scale = width / layout.viewportWidth;
  return { left: layout.left * scale, top: layout.top * scale, width: Math.max(1, layout.width * scale), height: Math.max(1, layout.height * scale) };
}
