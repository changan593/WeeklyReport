import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri 在开发时通过固定端口加载前端；构建产物在 dist/。
// 见 https://tauri.app/v1/guides/getting-started/setup/vite/
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: '127.0.0.1',
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: ['es2020', 'chrome105', 'safari13'],
    minify: 'esbuild',
    sourcemap: false,
  },
});
