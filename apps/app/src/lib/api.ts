import { createApi } from '@chaudron/api';

/** En production l'API est sur la même origine. En dev (Expo sur :8081), `EXPO_PUBLIC_API_URL=http://localhost:28471`. */
export const apiBase = process.env.EXPO_PUBLIC_API_URL ?? '';

export const api = createApi(apiBase);

/** Adresse de connexion classique (redirection vers le fournisseur). */
export function loginUrl(provider: string, options: { invite?: string; returnTo?: string } = {}): string {
  const query = new URLSearchParams();
  if (options.invite) query.set('invite', options.invite);
  if (options.returnTo) query.set('return_to', options.returnTo);
  const suffix = query.size ? `?${query}` : '';
  return `${apiBase}/api/v1/auth/${provider}/login${suffix}`;
}
