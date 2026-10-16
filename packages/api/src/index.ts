// Client de l'API Chaudron, typé par le contrat OpenAPI du serveur.
// openapi.json est écrit par le test Rust `openapi_json_a_jour` ; schema.d.ts est généré par `npm run api:generate`.
import createClient from 'openapi-fetch';
import type { components, paths } from './schema';

export type { paths } from './schema';
export type Schemas = components['schemas'];
export type Me = Schemas['Me'];
export type Household = Schemas['Household'];
export type HouseholdUser = Schemas['HouseholdUser'];
export type Member = Schemas['Member'];
export type MemberInput = Schemas['MemberInput'];
export type MemberKind = Schemas['MemberKind'];
export type Diet = Schemas['Diet'];
export type Portions = Schemas['Portions'];
export type Presence = Schemas['Presence'];
export type Exclusion = Schemas['Exclusion'];
export type ExclusionKind = Schemas['ExclusionKind'];
export type Equipment = Schemas['Equipment'];
export type Ingredient = Schemas['Ingredient'];
export type InviteInfo = Schemas['InviteInfo'];
export type PendingInvite = Schemas['PendingInvite'];
export type ProviderInfo = Schemas['ProviderInfo'];
export type Role = Schemas['Role'];
export type ErrorBody = Schemas['ErrorBody'];

/** `baseUrl` vide : l'API est sur la même origine (production). En dev, l'adresse du serveur Rust. */
export function createApi(baseUrl = '') {
  return createClient<paths>({ baseUrl, credentials: 'include' });
}

export type Api = ReturnType<typeof createApi>;

/** Message lisible d'une erreur de l'API (`{ error, message }`), sinon un message générique. */
export function errorMessage(error: unknown, fallback = 'Chaudron ne répond pas. Réessaie.'): string {
  if (error && typeof error === 'object' && 'message' in error && typeof error.message === 'string') {
    return error.message;
  }
  return fallback;
}
