import boundaries from '@boundaries/eslint-plugin'
import js from '@eslint/js'
import { defineConfig, globalIgnores } from 'eslint/config'
import reactHooks from 'eslint-plugin-react-hooks'
import globals from 'globals'
import tseslint from 'typescript-eslint'

export default defineConfig([
  // The generated bindings are checked in and verified by the Rust drift test.
  globalIgnores(['dist', 'src/shared/ipc/bindings.ts']),
  {
    files: ['**/*.{ts,tsx,js}'],
    extends: [js.configs.recommended],
    languageOptions: { ecmaVersion: 2023, globals: { ...globals.browser } },
  },
  {
    files: ['**/*.{ts,tsx}'],
    extends: [tseslint.configs.recommendedTypeChecked, reactHooks.configs.flat.recommended],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
  {
    files: ['vite.config.ts', 'eslint.config.js'],
    languageOptions: { globals: { ...globals.node } },
  },
  {
    files: ['src/**/*.{ts,tsx}'],
    plugins: { boundaries },
    settings: {
      'import/resolver': {
        typescript: { alwaysTryTypes: true, project: './tsconfig.app.json' },
      },
      'boundaries/include': ['src/**/*'],
      // The first matching element wins, so the narrower `shared-ipc` comes before `shared`.
      'boundaries/elements': [
        { type: 'app', pattern: 'src/app' },
        { type: 'feature', pattern: 'src/features/*', capture: ['name'] },
        { type: 'shared-ipc', pattern: 'src/shared/ipc' },
        { type: 'shared', pattern: 'src/shared' },
      ],
    },
    rules: {
      'boundaries/no-unknown-files': 2,
      'boundaries/no-unknown-dependencies': 2,
      'boundaries/dependencies': [
        2,
        {
          default: 'disallow',
          // Without this, external packages (such as @tauri-apps/api) are never evaluated.
          checkAllOrigins: true,
          policies: [
            // Third-party and Node packages are open by default.
            { allow: { to: { module: { origin: ['external', 'core'] } } } },
            // app -> features, shared
            {
              from: { element: { type: 'app' } },
              allow: {
                to: [
                  { element: { type: 'app' } },
                  { element: { type: 'feature' } },
                  { element: { type: 'shared' } },
                  { element: { type: 'shared-ipc' } },
                ],
              },
            },
            // feature -> shared, itself (never another feature)
            {
              from: { element: { type: 'feature' } },
              allow: {
                to: [
                  {
                    element: {
                      type: 'feature',
                      captured: { name: '{{from.element.captured.name}}' },
                    },
                  },
                  { element: { type: 'shared' } },
                  { element: { type: 'shared-ipc' } },
                ],
              },
            },
            // shared -> shared
            {
              from: { element: { type: ['shared', 'shared-ipc'] } },
              allow: { to: [{ element: { type: 'shared' } }, { element: { type: 'shared-ipc' } }] },
            },
            // Generated bindings and the Tauri API stay behind src/shared/ipc.
            {
              from: { element: { type: ['app', 'feature', 'shared'] } },
              disallow: {
                to: { element: { type: 'shared-ipc', fileInternalPath: 'bindings.ts' } },
              },
            },
            {
              from: { element: { type: ['app', 'feature', 'shared'] } },
              disallow: { to: { module: { origin: 'external', source: '@tauri-apps/*' } } },
            },
          ],
        },
      ],
    },
  },
])
