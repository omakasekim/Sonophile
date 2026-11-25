// Cross-origin isolation setup (non-blocking so hard-refresh never hangs initApp).

export async function setupCrossOriginIsolation(): Promise<boolean> {
  updateBadge();
  if (typeof window === 'undefined' || !('serviceWorker' in navigator)) {
    return typeof window !== 'undefined' && window.crossOriginIsolated === true;
  }

  if (window.crossOriginIsolated) {
    updateBadge();
    return true;
  }

  try {
    const sw = navigator.serviceWorker;
    const reg = await sw.register('./coi-serviceworker.js', { scope: './' });
    // Only reload once on brand-new install (never block forever on hard refresh where sw.controller is null).
    if (!sw.controller && reg.installing) {
      const reloadKey = 'sonophile_coi_reloaded';
      if (!sessionStorage.getItem(reloadKey)) {
        const waitCtrl = new Promise<boolean>((resolve) => {
          const timer = setTimeout(() => resolve(false), 1200);
          sw.addEventListener(
            'controllerchange',
            () => {
              clearTimeout(timer);
              resolve(true);
            },
            { once: true }
          );
        });
        if (await waitCtrl) {
          sessionStorage.setItem(reloadKey, '1');
          location.reload();
        }
      }
    }
  } catch (err) {
    console.warn('[coi] service worker isolation unavailable:', err);
  }
  updateBadge();
  return Boolean(window.crossOriginIsolated);
}

function updateBadge(): void {
  const el = document.getElementById('isolation-badge');
  if (el) {
    const isolated = typeof window !== 'undefined' && window.crossOriginIsolated;
    el.textContent = isolated ? 'crossOriginIsolated ✓' : 'single-thread mode ✓';
  }
}

export const isCrossOriginIsolated = (): boolean =>
  typeof window !== 'undefined' && window.crossOriginIsolated === true;
