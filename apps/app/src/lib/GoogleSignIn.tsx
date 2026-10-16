// Connexion Google sur le web : bouton officiel de Google et One Tap (Google Identity Services).
// Le jeton reçu de Google est vérifié par le serveur (POST /api/v1/auth/google/one-tap).
// Si le script de Google ne se charge pas (bloqueur, hors ligne), bouton de connexion classique par redirection.
import { errorMessage } from '@chaudron/api';
import { type ComponentRef, useEffect, useRef, useState } from 'react';
import { Platform, View } from 'react-native';

import { Button } from '../ui';
import { api, loginUrl } from './api';
import { useTheme } from './theme';

interface GoogleId {
  initialize: (options: Record<string, unknown>) => void;
  renderButton: (parent: HTMLElement, options: Record<string, unknown>) => void;
  prompt: () => void;
  cancel: () => void;
}

declare global {
  interface Window {
    google?: { accounts: { id: GoogleId } };
  }
}

const SCRIPT = 'https://accounts.google.com/gsi/client';
let loading: Promise<GoogleId> | null = null;

function loadGoogle(): Promise<GoogleId> {
  loading ??= new Promise((resolve, reject) => {
    const script = document.createElement('script');
    script.src = SCRIPT;
    script.async = true;
    script.onload = () => (window.google ? resolve(window.google.accounts.id) : reject(new Error('google')));
    script.onerror = () => {
      loading = null;
      reject(new Error('google'));
    };
    document.head.append(script);
  });
  return loading;
}

export function GoogleSignIn({
  clientId,
  invite,
  onLoggedIn,
  onError,
}: {
  clientId: string;
  invite?: string;
  onLoggedIn: (returnTo: string) => void;
  onError: (message: string) => void;
}) {
  const { mode } = useTheme();
  // Web : la View est un élément du DOM, où Google dessine son bouton.
  const container = useRef<ComponentRef<typeof View>>(null);
  const [fallback, setFallback] = useState(Platform.OS !== 'web');
  const callbacks = useRef({ onLoggedIn, onError });
  useEffect(() => {
    callbacks.current = { onLoggedIn, onError };
  });

  useEffect(() => {
    if (Platform.OS !== 'web') return;
    let cancelled = false;
    loadGoogle()
      .then((google) => {
        if (cancelled) return;
        google.initialize({
          client_id: clientId,
          ux_mode: 'popup',
          use_fedcm_for_prompt: true,
          itp_support: true,
          context: 'signin',
          callback: async ({ credential }: { credential: string }) => {
            const { data, error } = await api.POST('/api/v1/auth/google/one-tap', {
              body: { credential, invite: invite ?? null },
            });
            if (data) callbacks.current.onLoggedIn(data.return_to);
            else callbacks.current.onError(errorMessage(error));
          },
        });
        const parent = container.current as unknown as HTMLElement | null;
        if (parent) {
          google.renderButton(parent, {
            theme: mode === 'dark' ? 'filled_black' : 'outline',
            size: 'large',
            shape: 'pill',
            text: 'continue_with',
            logo_alignment: 'center',
            locale: 'fr',
            width: Math.min(parent.clientWidth || 320, 400),
          });
        }
        google.prompt();
      })
      .catch(() => !cancelled && setFallback(true));
    return () => {
      cancelled = true;
      window.google?.accounts.id.cancel();
    };
  }, [clientId, invite, mode]);

  if (fallback) {
    return (
      <Button
        label="Continuer avec Google"
        onPress={() => {
          globalThis.location?.assign(loginUrl('google', { invite }));
        }}
      />
    );
  }
  return <View ref={container} style={{ minHeight: 44, alignItems: 'center' }} />;
}
