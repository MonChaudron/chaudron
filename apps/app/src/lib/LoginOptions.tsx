import { useRouter } from 'expo-router';
import { useState } from 'react';

import { Button, ErrorText, Stack } from '../ui';
import { loginUrl } from './api';
import { GoogleSignIn } from './GoogleSignIn';
import { useSession } from './session';

const labels: Record<string, string> = {
  amazon: 'Continuer avec Amazon',
  google: 'Continuer avec Google',
  fake: 'Connexion factice (dev)',
};

/**
 * Boutons de connexion des fournisseurs configurés. Avec `invite`, la connexion fait entrer dans le foyer.
 * `initialError` : erreur d'une connexion précédente (redirection), remplacée par celle de One Tap s'il y en a une.
 */
export function LoginOptions({ invite, initialError }: { invite?: string; initialError?: string | null }) {
  const { providers, refresh } = useSession();
  const router = useRouter();
  const [oneTapError, setError] = useState<string | null>(null);
  const error = oneTapError ?? initialError ?? null;
  const go = (provider: string) => globalThis.location?.assign(loginUrl(provider, { invite }));

  return (
    <Stack>
      <ErrorText>{error}</ErrorText>
      {providers.map((provider) =>
        provider.id === 'google' && provider.client_id ? (
          <GoogleSignIn
            key="google"
            clientId={provider.client_id}
            invite={invite}
            onError={setError}
            onLoggedIn={async (returnTo) => {
              await refresh();
              router.replace(returnTo as never);
            }}
          />
        ) : (
          <Button key={provider.id} label={labels[provider.id] ?? provider.id} onPress={() => go(provider.id)} />
        ),
      )}
    </Stack>
  );
}

/** Messages des erreurs de connexion renvoyées par le serveur (`/connexion?erreur=…`). */
export const loginErrors: Record<string, string> = {
  refus: 'Connexion annulée.',
  fournisseur: 'Le fournisseur de connexion a refusé ou ne répond pas. Réessaie.',
  expiree: 'La connexion a pris trop de temps. Réessaie.',
  'inscriptions-fermees': 'Les inscriptions sont fermées : il faut un lien d’invitation.',
  'deja-lie': 'Ce compte est déjà lié à un autre compte Chaudron.',
  interne: 'Erreur du serveur. Réessaie plus tard.',
};
