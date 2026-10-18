// Libellés français des codes de l'interface (`adult`, `equipment.oven`…) : l'i18n vit ici. Les ingrédients ont
// leur nom en base.

export const memberKinds = {
  adult: 'Adulte',
  teen: 'Ado',
  child: 'Enfant',
} as const;

export const diets = {
  omnivore: 'Mange de tout',
  pescatarian: 'Pescétarien',
  vegetarian: 'Végétarien',
  vegan: 'Végétalien',
} as const;

export const meals = {
  lunch: 'Midi',
  dinner: 'Soir',
} as const;

/** Du lundi au dimanche, comme `Presence` (indice 0 = lundi). */
export const weekdays = ['lundi', 'mardi', 'mercredi', 'jeudi', 'vendredi', 'samedi', 'dimanche'] as const;
export const weekdaysShort = ['lun.', 'mar.', 'mer.', 'jeu.', 'ven.', 'sam.', 'dim.'] as const;
export const weekdaysLetter = ['L', 'M', 'M', 'J', 'V', 'S', 'D'] as const;

export const roles = {
  owner: 'Propriétaire',
  member: 'Membre',
} as const;

export const exclusionKinds = {
  allergy: 'Allergie',
  dislike: 'N’aime pas',
} as const;

/** Rôles d'ingrédient du barème (design §3). */
export const portionRoles = {
  protein: 'Protéines',
  starch: 'Féculents',
  veg: 'Légumes',
  fat: 'Matières grasses',
  other: 'Le reste',
} as const;

export const equipment: Record<string, string> = {
  'equipment.thermomix_tm6': 'Thermomix TM6',
  'equipment.cookeo_6l': 'Cookeo 6 L',
  'equipment.pressure_cooker': 'Autocuiseur',
  'equipment.oven': 'Four',
  'equipment.hob': 'Plaques',
  'equipment.microwave': 'Micro-ondes',
  'equipment.airfryer': 'Airfryer',
  'equipment.blender': 'Blender',
};

/** Libellé d'une clé de traduction, ou la clé elle-même si elle est inconnue (nouvel ingrédient). */
export function label(dictionary: Record<string, string>, key: string): string {
  return dictionary[key] ?? key.replace(/^[a-z]+\./, '').replaceAll('_', ' ');
}

/** Forme d'achat d'un ingrédient. */
export const ingredientStates = {
  fresh: 'Frais',
  frozen: 'Surgelé',
  canned: 'En conserve',
  dried: 'Sec',
  cooked: 'Cuit',
} as const;

/** Droits d'un jeton d'API. */
export const scopes = {
  read: 'Lire les recettes et les menus',
  'recipes:write': 'Publier des recettes',
  'menus:write': 'Publier des menus',
  'cooking:write': 'Avancer dans la cuisson',
} as const;

/** Unités, pour la saisie. */
export const units = {
  g: 'g',
  ml: 'ml',
  piece: 'pièce',
  tbsp: 'c. à s.',
  tsp: 'c. à c.',
  pinch: 'pincée',
} as const;

export const aisles = {
  produce: 'Fruits et légumes',
  butcher: 'Boucherie',
  fish: 'Poissonnerie',
  dairy: 'Crèmerie',
  fresh: 'Frais',
  frozen: 'Surgelés',
  grocery: 'Épicerie',
  bakery: 'Boulangerie',
  other: 'Autre',
} as const;
