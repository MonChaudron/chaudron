// Prix estimés (design-prix.md) : des moyennes, jamais le ticket de caisse. Un total s'affiche à l'euro, avec
// « ≈ » ; le prix d'un article ou d'une portion, au centime.

export type PriceTier = 'low' | 'mid' | 'high';

const euros = new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 0 });
const cents = new Intl.NumberFormat('fr-FR', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/** Un total : « ≈ 148 € ». */
export function formatTotal(amount: number): string {
  return `≈ ${euros.format(Math.round(amount / 100))} €`;
}

/** Le prix d'un article, d'une portion : « 4,30 € ». */
export function formatPrice(amount: number): string {
  return `${cents.format(amount / 100)} €`;
}

/** Le prix d'un plat : « ≈ 19 € », « ≈ 19 € + » s'il manque des prix, « prix inconnu » s'il n'en a aucun. */
export function pillText(amount: number, complete: boolean): string {
  if (!complete && amount === 0) return 'prix inconnu';
  return complete ? formatTotal(amount) : `${formatTotal(amount)} +`;
}

/** Sous-total de chaque rayon, et du placard, des articles qui ont un prix. */
export function aisleSubtotals(items: { aisle: string; pantry: boolean; price_cents?: number | null }[]): Record<string, number> {
  const totals: Record<string, number> = {};
  for (const item of items) {
    if (item.price_cents == null) continue;
    const section = item.pantry ? 'pantry' : item.aisle;
    totals[section] = (totals[section] ?? 0) + item.price_cents;
  }
  return totals;
}

/** Où se place un plat parmi les recettes du foyer. */
export function tierLabel(tier: PriceTier): string {
  return { low: 'bon marché', mid: 'dans la moyenne', high: 'cher' }[tier];
}
