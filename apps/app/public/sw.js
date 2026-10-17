// Service worker de Chaudron : l'appli s'ouvre sans réseau (au magasin, maison coupée d'internet).
// Écrit à la main, sans bibliothèque. `scripts/pwa.mjs` y inscrit la version et la liste des fichiers à chaque build :
// un nouveau build change ce fichier, le navigateur installe la nouvelle version.
const VERSION = '__VERSION__';
const ASSETS = [] /* __ASSETS__ */;
const CACHE = `chaudron-${VERSION}`;

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(['/', ...ASSETS]))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k.startsWith('chaudron-') && k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (event) => {
  const request = event.request;
  const url = new URL(request.url);
  // L'API n'est jamais mise en cache ici : l'appli garde sa propre copie de travail (IndexedDB).
  if (request.method !== 'GET' || url.origin !== self.location.origin || url.pathname.startsWith('/api/') || url.pathname.startsWith('/dev/')) return;

  // Une page : le réseau d'abord (nouvelle version), sinon l'appli gardée.
  if (request.mode === 'navigate') {
    event.respondWith(
      fetch(request)
        .then((response) => {
          if (response.ok) {
            const copy = response.clone();
            void caches.open(CACHE).then((cache) => cache.put('/', copy));
          }
          return response;
        })
        .catch(() => caches.match('/')),
    );
    return;
  }

  // Fichiers de l'appli (hachés, polices, icônes) : le cache d'abord.
  event.respondWith(
    caches.match(request).then(
      (hit) =>
        hit ||
        fetch(request).then((response) => {
          if (response.ok) {
            const copy = response.clone();
            void caches.open(CACHE).then((cache) => cache.put(request, copy));
          }
          return response;
        }),
    ),
  );
});
