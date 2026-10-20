import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import globals from 'globals';
import reactHooks from 'eslint-plugin-react-hooks';
import tseslint from 'typescript-eslint';

export default defineConfig([
  globalIgnores([
    '.cache/**', '.expo/**', 'android/**', 'ios/**', 'dist/**', 'target/**',
    'modules/govue-core/generated/**', 'modules/govue-core/ios/generated/**',
    'src/youtube/generated/**',
    // Vendored DASH player and browser extension sources.
    'core/src/cast/assets/dash.all.min.js', 'src/youtube/litube/**',
  ]),
  {
    files: ['**/*.{js,jsx,cjs,mjs,ts,tsx}'],
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
    files: ['**/*.{ts,tsx}'],
    extends: [tseslint.configs.recommended],
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { ignoreRestSiblings: true, argsIgnorePattern: '^_' }],
    },
  },
  {
    files: ['core/src/cast/assets/player.js'],
    languageOptions: { globals: { dashjs: 'readonly' } },
  },
  {
    files: ['App.tsx', 'src/**/*.{ts,tsx}'],
    plugins: { 'react-hooks': reactHooks },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
    },
  },
]);
