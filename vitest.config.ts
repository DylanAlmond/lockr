import { defineConfig } from 'vitest/config';

/**
 * The frontend harness covers pure logic only: it does not mount components
 * and does not mock Tauri's IPC boundary. A Node environment is therefore all
 * it needs, and it deliberately does not load the app's Vite plugins.
 */
export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts']
  }
});
