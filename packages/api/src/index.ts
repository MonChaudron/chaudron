// Client de l'API Chaudron, typé par le contrat OpenAPI du serveur.
// openapi.json est écrit par le test Rust `openapi_json_a_jour` ; schema.d.ts est généré par `npm run api:generate`.
import createClient from 'openapi-fetch';
import type { components, paths } from './schema';

export type { paths } from './schema';
export type Schemas = components['schemas'];
export type Me = Schemas['Me'];
export type SignupInfo = Schemas['SignupInfo'];
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
export type Recipe = Schemas['Recipe'];
export type RecipeSummary = Schemas['RecipeSummary'];
export type RecipeLine = Schemas['RecipeLine'];
export type Variant = Schemas['Variant'];
export type Phase = Schemas['Phase'];
export type Step = Schemas['Step'];
export type IngredientState = Schemas['IngredientState'];
export type Unit = Schemas['Unit'];
export type Scope = Schemas['Scope'];
export type ApiTokenInfo = Schemas['ApiTokenInfo'];
export type CreatedApiToken = Schemas['CreatedApiToken'];
export type LinkInfo = Schemas['LinkInfo'];
export type ClientInfo = Schemas['ClientInfo'];
export type Cooking = Schemas['Cooking'];
export type CookingInput = Schemas['CookingInput'];
export type RecipeInput = Schemas['RecipeInput'];
export type VersionInfo = Schemas['VersionInfo'];
export type RecipeVersion = Schemas['RecipeVersion'];
export type RecipeWarning = Schemas['RecipeWarning'];
export type Feedback = Schemas['Feedback'];
export type FeedbackInput = Schemas['FeedbackInput'];
export type Rating = Schemas['Rating'];
export type RatingCounts = Schemas['RatingCounts'];
export type Food = Schemas['Food'];
export type Aisle = Schemas['Aisle'];
export type PortionRole = Schemas['PortionRole'];
export type Meal = Schemas['Meal'];
export type Menu = Schemas['Menu'];
export type MenuSlot = Schemas['MenuSlot'];
export type MenuSummary = Schemas['MenuSummary'];
export type MenuInput = Schemas['MenuInput'];
export type SlotInput = Schemas['SlotInput'];
export type MoveMode = Schemas['MoveMode'];
export type Warning = Schemas['Warning'];
export type Agenda = Schemas['Agenda'];
export type ShoppingList = Schemas['ShoppingList'];
export type ShoppingItem = Schemas['ShoppingItem'];
export type ShoppingOp = Schemas['ShoppingOp'];
export type Block = Schemas['Block'];
export type OpKind = Schemas['OpKind'];

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
