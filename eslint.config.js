import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import globals from 'globals';

export default defineConfig([
  globalIgnores([
    '.cache/**', 'target/**', 'native-shell/**/build/**',
    'src/youtube/generated/**',
    // Vendored DASH player and browser extension sources.
    'core/src/cast/assets/dash.all.min.js', 'src/youtube/litube/**',
    // The launch film vendors its pinned WebGL runtime and generates render artifacts.
    'videos/**/assets/vendor/**', 'videos/**/.hyperframes/**', 'videos/**/renders/**',
  ]),
  {
    files: ['**/*.{js,cjs,mjs}'],
    extends: [js.configs.recommended],
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
    rules: {
      'no-empty': ['error', { allowEmptyCatch: true }],
      'no-unused-vars': ['error', { ignoreRestSiblings: true, argsIgnorePattern: '^_' }],
    },
  },
  {
    files: ['core/src/cast/assets/player.js'],
    languageOptions: { globals: { dashjs: 'readonly' } },
  },
]);
