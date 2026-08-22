// FCM background-message handler, registered at a custom scope
// ('/firebase-cloud-messaging-push-scope') so it can coexist with the PWA's
// own workbox-managed service worker at the root scope — Firebase's
// documented approach for apps that already have another SW controlling '/'.
// See src/hooks/usePushNotifications.tsx for the registration call.
//
// This is a plain static asset (not bundled by Vite), so it can't import
// src/lib/firebase.ts's runtime env. Instead it reads the (public,
// client-safe) Firebase config back off its own registration URL — passed as
// a query string by the code that calls `serviceWorkerRegistration.register`.
importScripts('https://www.gstatic.com/firebasejs/12.3.0/firebase-app-compat.js');
importScripts('https://www.gstatic.com/firebasejs/12.3.0/firebase-messaging-compat.js');

const params = new URLSearchParams(self.location.search);
firebase.initializeApp({
  apiKey: params.get('apiKey'),
  authDomain: params.get('authDomain'),
  projectId: params.get('projectId'),
  storageBucket: params.get('storageBucket'),
  messagingSenderId: params.get('messagingSenderId'),
  appId: params.get('appId'),
});

const messaging = firebase.messaging();

// Only fires while the app isn't in the foreground (tab hidden/closed) — the
// foreground case is handled in-app via `onMessage` (see
// usePushNotifications.tsx), which just refreshes the notification queries
// instead of popping a duplicate system notification.
//
// The backend (agon_core::push::PushClient::send) deliberately sends a
// data-only message — title/body/link all under `payload.data`, nothing
// under `payload.notification` — because a `notification` payload gets
// auto-displayed by the browser using its own default handling whenever the
// page isn't foregrounded, bypassing this handler entirely (so a click on it
// couldn't carry `link` anywhere). Data-only always reaches us here.
messaging.onBackgroundMessage((payload) => {
  const { title, body, link } = payload.data || {};
  if (!title) return;
  self.registration.showNotification(title, {
    body,
    icon: '/pwa-192x192.png',
    // Read back in notificationclick below. `link` is absent when the
    // backend has no AGON_UI_URL configured — falls back to the app root.
    data: { link },
  });
});

// Focus an existing tab already on that link, or open one, on click.
self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const url = event.notification.data?.link || self.location.origin + '/';
  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((clients) => {
      for (const client of clients) {
        if (client.url === url && 'focus' in client) return client.focus();
      }
      if (self.clients.openWindow) return self.clients.openWindow(url);
    }),
  );
});
