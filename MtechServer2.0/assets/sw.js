// KILL-SWITCH service worker. Replaces the legacy 'mtechserver-pwa'
// cache-first worker on stale clients: deletes every cache, unregisters
// itself, and reloads controlled tabs so they fetch the live app from the
// network. The current app registers no service worker — this file exists
// only so old registrations polling /sw.js pick it up and self-destruct.
self.addEventListener("install", () => self.skipWaiting());

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) {
        await caches.delete(key);
      }
      await self.clients.claim();
      await self.registration.unregister();
      const tabs = await self.clients.matchAll({ type: "window" });
      for (const tab of tabs) {
        try {
          tab.navigate(tab.url);
        } catch (_) {
          // Uncontrollable tab — it already hits the network next load.
        }
      }
    })()
  );
});
// Deliberately NO 'fetch' handler: pages fall straight through to network.
