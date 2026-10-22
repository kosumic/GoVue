import { readFileSync, writeFileSync, existsSync } from 'node:fs';

// Temporary backports while these advisories have no patched npm release.
// Exact versions and source anchors make upstream changes require review.
const lock = JSON.parse(readFileSync('package-lock.json', 'utf8')) as {
  packages: Record<string, { version?: string }>;
};

function locations(name: string, version: string): string[] {
  const directories = Object.keys(lock.packages).filter(path => path.endsWith(`node_modules/${name}`));
  if (!directories.length) throw Error(`Review the ${name} security backport; dependency removed.`);
  for (const directory of directories) {
    const installed = JSON.parse(readFileSync(`${directory}/package.json`, 'utf8')) as { version: string };
    if (installed.version !== version || lock.packages[directory].version !== version) {
      throw Error(`Review the ${name} security backport for the new version.`);
    }
  }
  return directories;
}

function patch(path: string, before: string, after: string) {
  const source = readFileSync(path, 'utf8');
  if (source.includes(after)) return;
  if (source.split(before).length !== 2) throw Error(`Review the security backport in ${path}.`);
  writeFileSync(path, source.replace(before, after));
}

// GHSA-vfj7-8cjw-p6xm: bound both parsed nesting and caller-supplied ASTs.
// https://github.com/micromatch/braces/pull/88
// Backported from a795ee83ecd011131b2d618fe754ac5a8e124dae.
const assertDepth = `'use strict';
module.exports = ast => {
  const pending = [[ast, 0, false]];
  const seen = new WeakSet();
  while (pending.length) {
    const [node, depth, exit] = pending.pop();
    if (exit) { seen.delete(node); continue; }
    if (depth > 128) throw new SyntaxError('Brace AST exceeds safe depth of 128');
    if (seen.has(node)) throw new SyntaxError('Brace AST must be an acyclic tree');
    seen.add(node);
    pending.push([node, depth, true]);
    if (node.nodes) for (const child of node.nodes) pending.push([child, depth + 1, false]);
  }
};
`;
for (const directory of locations('braces', '3.0.3')) {
  const path = `${directory}/lib/assert-depth.js`;
  if (existsSync(path) && readFileSync(path, 'utf8') !== assertDepth) {
    throw Error(`Review the existing brace depth guard in ${path}.`);
  }
  writeFileSync(path, assertDepth);
  for (const opening of ['CHAR_LEFT_PARENTHESES', 'CHAR_LEFT_CURLY_BRACE']) {
    const before = `    if (value === ${opening}) {`;
    patch(`${directory}/lib/parse.js`, before, `${before}
      if (stack.length > 128) {
        throw new SyntaxError('Brace AST exceeds safe depth of 128');
      }`);
  }
  for (const [file, declaration] of [
    ['compile', 'const compile = (ast, options = {}) => {'],
    ['expand', 'const expand = (ast, options = {}) => {'],
    ['stringify', 'module.exports = (ast, options = {}) => {'],
  ]) {
    patch(`${directory}/lib/${file}.js`, declaration, `${declaration}\n  require('./assert-depth')(ast);`);
  }
}

// GHSA-86w9-cpqp-85rv: reject extra nested DigestAlgorithm elements.
// https://github.com/digitalbazaar/forge/pull/1152
// Backported from ceba34402e329f0365134f23fe19898756527d65.
for (const directory of locations('node-forge', '1.4.0')) {
  patch(`${directory}/lib/rsa.js`,
    '            obj.value.length !== 2) {',
    `            obj.value.length !== 2 ||
            obj.value[0].value.length !==
              (('parameters' in capture) ? 2 : 1)) {`);
}
