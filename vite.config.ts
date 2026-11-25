import { defineConfig } from 'vite';

// `base: './'` makes every asset resolve relative to the document, so the build
// works both at the repository root, under a GitHub Pages subpath
// (e.g. `/<repo-name>/`), and on a custom domain — without any host-specific
// rewrite rules. `AudioWorklet` modules are emitted as separate assets and
// loaded by URL, so they honour the same relative base.
export default defineConfig({
  base: './',
  build: {
    target: 'es2020',
    outDir: 'dist',
    // Emit the wasm and worklet files as real files (never inlined), so the
    // service worker can serve them with the required CORP headers.
    assetsInlineLimit: 0,
    sourcemap: false,
  },
  worker: {
    format: 'es',
  },
});
