import { readFileSync, writeFileSync } from 'node:fs';

// WebView 14.0.1 intersects its props with a default generic of undefined,
// reducing every JSX prop to never under strictNullChecks. An empty object
// preserves the declared props when callers do not add their own generic.
const path = 'node_modules/react-native-webview/index.d.ts';
const before = 'declare class WebView<P = undefined>';
const after = 'declare class WebView<P = {}>';
const source = readFileSync(path, 'utf8');
if (!source.includes(after)) {
  if (!source.includes(before))
    throw Error('WebView declaration changed; review the default props patch.');
  writeFileSync(path, source.replace(before, after));
}
