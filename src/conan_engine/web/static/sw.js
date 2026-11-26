const SHELL_CACHE = 'conan-tcg-shell-v4';
const IMAGE_CACHE = 'conan-tcg-images-v1';
const MAX_IMAGE_ENTRIES = 300;

const APP_SHELL_URLS = [
  '/',
  '/index.html',
  '/manifest.webmanifest',
  '/pwa.js',
  '/cardSE.js',
  '/offline.html',
  '/icons/icon-192.png',
  '/icons/icon-512.png',
  '/icons/icon.svg',
];

async function trimImageCache(cache) {
  const keys = await cache.keys();
  if (keys.length <= MAX_IMAGE_ENTRIES) return;
  const deleteCount = keys.length - MAX_IMAGE_ENTRIES;
  await Promise.all(keys.slice(0, deleteCount).map((key) => cache.delete(key)));
}

self.addEventListener('install', (event) => {
  event.waitUntil((async () => {
    const cache = await caches.open(SHELL_CACHE);
    await Promise.allSettled(
      APP_SHELL_URLS.map(async (url) => {
        try {
          const response = await fetch(new Request(url, { cache: 'reload' }));
          if (response && response.ok) {
            await cache.put(url, response);
          } else {
            console.warn('[sw] App Shell プリキャッシュ失敗:', url, response && response.status);
          }
        } catch (err) {
          console.warn('[sw] App Shell プリキャッシュ例外:', url, err);
        }
      })
    );
  })());
});

self.addEventListener('activate', (event) => {
  const validCaches = new Set([SHELL_CACHE, IMAGE_CACHE]);
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(
        keys.filter((key) => !validCaches.has(key)).map((key) => caches.delete(key))
      ))
      .then(() => self.clients.claim())
  );
});

self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    self.skipWaiting();
  }
});

self.addEventListener('fetch', (event) => {
  const { request } = event;
  if (request.method !== 'GET') return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  // API やヘルスチェックは絶対にキャッシュしない
  if (url.pathname.startsWith('/api/') || url.pathname === '/health') {
    return;
  }

  // Range リクエスト（音声ストリーミング等）は介入せずネットワークへパスルー
  if (request.headers.has('range')) {
    return;
  }

  // ナビゲーション: Network-first（失敗時は / または /offline.html へフォールバック）
  if (request.mode === 'navigate') {
    event.respondWith((async () => {
      try {
        const networkResponse = await fetch(request);
        if (networkResponse && networkResponse.ok) {
          const cache = await caches.open(SHELL_CACHE);
          cache.put(request, networkResponse.clone()).catch(() => {});
        }
        return networkResponse;
      } catch (_err) {
        const cache = await caches.open(SHELL_CACHE);
        const cached = (await cache.match(request))
          || (await cache.match('/'))
          || (await cache.match('/index.html'))
          || (await cache.match('/offline.html'));
        if (cached) return cached;
        return new Response('オフラインです', {
          status: 503,
          headers: { 'Content-Type': 'text/plain; charset=utf-8' },
        });
      }
    })());
    return;
  }

  // カード画像: Cache-first + 最大300件トリム
  if (url.pathname.startsWith('/card-images/')) {
    event.respondWith((async () => {
      const cache = await caches.open(IMAGE_CACHE);
      const cached = await cache.match(request);
      if (cached) {
        return cached;
      }
      const networkResponse = await fetch(request);
      if (networkResponse && networkResponse.ok) {
        await cache.put(request, networkResponse.clone());
        await trimImageCache(cache);
      }
      return networkResponse;
    })());
    return;
  }

  // その他の同一オリジン静的アセット: Stale-while-revalidate
  event.respondWith((async () => {
    const cache = await caches.open(SHELL_CACHE);
    const cached = await cache.match(request);
    const networkPromise = fetch(request)
      .then((networkResponse) => {
        if (networkResponse && networkResponse.ok) {
          cache.put(request, networkResponse.clone()).catch(() => {});
        }
        return networkResponse;
      })
      .catch(() => null);

    if (cached) {
      event.waitUntil(networkPromise);
      return cached;
    }
    const networkResponse = await networkPromise;
    if (networkResponse) return networkResponse;
    return new Response('', { status: 504, statusText: 'Gateway Timeout' });
  })());
});
