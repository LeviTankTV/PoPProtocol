// PoP - Protocol: офлайн-кэш оболочки (клиент всегда берём свежий с сервера).
const CACHE = "pop-shell-v1";
const SHELL = ["/", "/index.html", "/icon.svg", "/manifest.json"];
self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});
self.addEventListener("activate", (e) => {
  e.waitUntil(caches.keys().then((ks) => Promise.all(ks.filter((k) => k !== CACHE).map((k) => caches.delete(k)))).then(() => self.clients.claim()));
});
self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (url.pathname.startsWith("/ws")) return;
  // app.js/proto.js/карты/статика: сначала сеть, при офлайне — кэш
  e.respondWith(
    fetch(e.request)
      .then((res) => {
        if (res.ok && url.pathname !== "/") {
          const copy = res.clone();
          caches.open(CACHE).then((c) => c.put(e.request, copy));
        }
        return res;
      })
      .catch(() => caches.match(e.request).then((m) => m || caches.match("/index.html")))
  );
});
