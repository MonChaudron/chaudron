// Libellés français. Le serveur ne stocke que des codes (`ingredient.peanut`, `adult`…) : l'i18n vit ici.

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
};

export const ingredients: Record<string, string> = {
  'ingredient.peanut': 'Arachide',
  'ingredient.hazelnut': 'Noisette',
  'ingredient.walnut': 'Noix',
  'ingredient.almond': 'Amande',
  'ingredient.milk': 'Lait',
  'ingredient.cream': 'Crème',
  'ingredient.butter': 'Beurre',
  'ingredient.egg': 'Œuf',
  'ingredient.wheat_flour': 'Farine de blé',
  'ingredient.pasta': 'Pâtes',
  'ingredient.rice': 'Riz',
  'ingredient.shrimp': 'Crevette',
  'ingredient.mussel': 'Moule',
  'ingredient.salmon': 'Saumon',
  'ingredient.cod': 'Cabillaud',
  'ingredient.tuna': 'Thon',
  'ingredient.anchovy': 'Anchois',
  'ingredient.celery': 'Céleri',
  'ingredient.mustard': 'Moutarde',
  'ingredient.sesame': 'Sésame',
  'ingredient.tofu': 'Tofu',
  'ingredient.mushroom': 'Champignon',
  'ingredient.bell_pepper': 'Poivron',
  'ingredient.onion': 'Oignon',
  'ingredient.garlic': 'Ail',
  'ingredient.coriander': 'Coriandre',
  'ingredient.spinach': 'Épinard',
  'ingredient.cauliflower': 'Chou-fleur',
  'ingredient.broccoli': 'Brocoli',
  'ingredient.zucchini': 'Courgette',
  'ingredient.eggplant': 'Aubergine',
  'ingredient.tomato': 'Tomate',
  'ingredient.carrot': 'Carotte',
  'ingredient.potato': 'Pomme de terre',
  'ingredient.beetroot': 'Betterave',
  'ingredient.endive': 'Endive',
  'ingredient.leek': 'Poireau',
  'ingredient.olive': 'Olive',
  'ingredient.lentil': 'Lentilles',
  'ingredient.chickpea': 'Pois chiches',
  'ingredient.blue_cheese': 'Fromage bleu',
  'ingredient.goat_cheese': 'Fromage de chèvre',
  'ingredient.chicken': 'Poulet',
  'ingredient.minced_beef': 'Bœuf haché',
  'ingredient.pork': 'Porc',
  'ingredient.veal': 'Veau',
  'ingredient.lamb': 'Agneau',
  'ingredient.liver': 'Foie',
};

/** Libellé d'une clé de traduction, ou la clé elle-même si elle est inconnue (nouvel ingrédient). */
export function label(dictionary: Record<string, string>, key: string): string {
  return dictionary[key] ?? key.replace(/^[a-z]+\./, '').replaceAll('_', ' ');
}
