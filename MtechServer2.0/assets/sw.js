// Service Worker for MtechServer PWA
// IMPORTANT: Update this version string with each deployment!
// This forces the service worker to update and clear old caches.
const CACHE_VERSION = 'v1.0.0'; // <-- CHANGE THIS ON EACH DEPLOY
const CACHE_NAME = `mtechserver-${CACHE_VERSION}`;

// Files that should ALWAYS be fetched fresh (never cached by SW)
const NO_CACHE_PATTERNS = [
  /index\.html$/,
  /manifest\.json$/,
  /sw\.js$/,
];

// Files with hashes that can be cached forever
const IMMUTABLE_PATTERNS = [
  /\.wasm$/,
  /\.js$/,
];

// Install event - activate immediately
self.addEventListener('install', (event) => {
  console.log(`[SW] Installing ${CACHE_NAME}`);
  // Skip waiting to activate new service worker immediately
  self.skipWaiting();
});

// Activate event - clean up old caches
self.addEventListener('activate', (event) => {
  console.log(`[SW] Activating ${CACHE_NAME}`);
  event.waitUntil(
    caches.keys().then((cacheNames) => {
      return Promise.all(
        cacheNames
          .filter((name) => name.startsWith('mtechserver-') && name !== CACHE_NAME)
          .map((name) => {
            console.log(`[SW] Deleting old cache: ${name}`);
            return caches.delete(name);
          })
      );
    }).then(() => {
      // Take control of all clients immediately
      return self.clients.claim();
    })
  );
});

// Fetch event - smart caching strategy
self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);
  
  // Only handle same-origin requests
  if (url.origin !== location.origin) {
    return;
  }

  // Check if this URL should never be cached
  const shouldNeverCache = NO_CACHE_PATTERNS.some(pattern => pattern.test(url.pathname));
  if (shouldNeverCache) {
    // Network-only for critical files
    event.respondWith(
      fetch(event.request).catch(() => {
        // If offline, try cache as fallback for index.html
        if (url.pathname.endsWith('index.html') || url.pathname === '/') {
          return caches.match('/index.html');
        }
        return new Response('Offline', { status: 503 });
      })
    );
    return;
  }

  // Check if this is an immutable file (has hash in name)
  const isImmutable = IMMUTABLE_PATTERNS.some(pattern => pattern.test(url.pathname));
  
  if (isImmutable) {
    // Cache-first for immutable assets (they have unique hashes)
    event.respondWith(
      caches.match(event.request).then((cachedResponse) => {
        if (cachedResponse) {
          return cachedResponse;
        }
        return fetch(event.request).then((networkResponse) => {
          // Cache the new file
          if (networkResponse.ok) {
            const responseClone = networkResponse.clone();
            caches.open(CACHE_NAME).then((cache) => {
              cache.put(event.request, responseClone);
            });
          }
          return networkResponse;
        });
      })
    );
    return;
  }

  // Stale-while-revalidate for other assets (images, etc.)
  event.respondWith(
    caches.match(event.request).then((cachedResponse) => {
      const fetchPromise = fetch(event.request).then((networkResponse) => {
        if (networkResponse.ok) {
          const responseClone = networkResponse.clone();
          caches.open(CACHE_NAME).then((cache) => {
            cache.put(event.request, responseClone);
          });
        }
        return networkResponse;
      }).catch(() => cachedResponse);

      return cachedResponse || fetchPromise;
    })
  );
});

// Listen for messages from the main app
self.addEventListener('message', (event) => {
  if (event.data === 'skipWaiting') {
    self.skipWaiting();
  }
  if (event.data === 'clearCache') {
    caches.keys().then((names) => {
      names.forEach((name) => caches.delete(name));
    });
  }
});
