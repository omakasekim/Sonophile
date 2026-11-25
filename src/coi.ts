// Cross-origin isolation setup (the `coi-serviceworker` pattern).
//
// GitHub Pages cannot set arbitrary HTTP headers, so `SharedArrayBuffer` /
// multi-threaded WASM (which need `Cross-Origin-Opener-Policy: same-origin` and
// `Cross-Origin-Embedder-Policy: require-corp`) are enabled by a service worker
// that re-serves the *document* with those headers. The registration below:
//
//  * registers the worker on first visit,
//  * waits until it is active AND controlling before reloading exactly once,
//  * never reloads again (the `sw.controller` guard), so no reload loop,
//  * falls back silently when service workers are unavailable (private mode,
//    sandboxed iframes, `file://`) — the app then runs single-threaded with the
//    `postMessage` ring-buffer path instead of `SharedArrayBuffer`.

export async function setupCrossOriginIsolation(): Promise<boolean> {
  updateBadge();
  if (typeof window === 'undefined') return false;
  if (!('serviceWorker' in navigator)) {
    // No SW support: report current state and continue with the fallback path.
    return window.crossOriginIsolated;
  }

  const sw = navigator.serviceWorker;
  // Already isolated → nothing to do.
  if (window.crossOriginIsolated) {
    updateBadge();
    return true;
  }

  try {
    // Absolute-from-current-directory registration so it works under a subpath.
    const reg = await sw.register('./coi-serviceworker.js', { scope: './' });
    // If the worker was already controlling this page (a later visit) we do
    // NOT reload; if it just took control, reload exactly once.
    if (!sw.controller) {
      await new Promise<void>((resolve) => {
        const onController = () => {
          sw.removeEventListener('controllerchange', onController);
          resolve();
        };
        sw.addEventListener('controllerchange', onController);
      });
      if (!window.crossOriginIsolated) {
        location.reload();
      }
    }
    await reg; // keep reference
  } catch (err) {
    console.warn('[coi] service worker isolation unavailable:', err);
  }
  updateBadge();
  return window.crossOriginIsolated;
}

function updateBadge(): void {
  const el = document.getElementById('isolation-badge');
  if (el) {
    const isolated = typeof window !== 'undefined' && window.crossOriginIsolated;
    el.textContent = isolated ? 'crossOriginIsolated ✓' : 'isolation: fallback (single-thread)';
  }
}

export const isCrossOriginIsolated = (): boolean =>
  typeof window !== 'undefined' && window.crossOriginIsolated === true;
