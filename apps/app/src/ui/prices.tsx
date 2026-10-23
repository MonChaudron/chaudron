import type { ShoppingList } from '@chaudron/api';
import { formatPrice, formatTotal, pillText, priceTierColors, type PriceTier, shortDateText, space, splitTitle, tierLabel } from '@chaudron/domain';
import { Text, View } from 'react-native';

import { useTheme } from '../lib/theme';
import { Body, Card, Stack } from './index';

/** Pastille du prix d'un plat : verte, ambre ou rouge selon son tiers parmi les recettes du foyer ; neutre sinon. */
export function PricePill({ cents, tier, complete }: { cents: number; tier?: PriceTier | null; complete: boolean }) {
  const { palette, fonts, weights, radius, mode } = useTheme();
  const colors = tier ? priceTierColors[mode][tier] : { bg: palette.chip, fg: palette.text };
  const text = pillText(cents, complete);
  const label = !complete && cents === 0 ? 'Prix inconnu' : `Environ ${Math.round(cents / 100)} euros${tier ? `, ${tierLabel(tier)}` : ''}${complete ? '' : ', sans le prix de certains ingrédients'}`;
  return (
    <View accessibilityLabel={label} style={{ alignSelf: 'flex-start', backgroundColor: colors.bg, borderRadius: radius.tag, paddingVertical: 4, paddingHorizontal: 10 }}>
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 14.5, color: colors.fg, fontVariant: ['tabular-nums'] }}>{text}</Text>
    </View>
  );
}

type WeekPrice = NonNullable<ShoppingList['price']>;

/** En tête des courses : le total de la semaine, le prix par portion, chaque repas, et d'où viennent les prix. */
export function PriceRecap({ price }: { price: WeekPrice }) {
  const { palette, fonts, weights } = useTheme();
  if (!price.total_cents && price.missing) {
    return (
      <Card>
        <Body tone="muted" size={15}>Prix pas encore calculés : ils arrivent avec la prochaine mise à jour.</Body>
      </Card>
    );
  }
  const row = (left: string, right: string, key: string) => (
    <View key={key} style={{ flexDirection: 'row', gap: space[3] }}>
      <Body size={15} style={{ flex: 1 }}>{left}</Body>
      <Text style={{ fontFamily: fonts.text, fontSize: 15, color: palette.text, fontVariant: ['tabular-nums'] }}>{right}</Text>
    </View>
  );
  return (
    <Card style={{ gap: space[2], maxWidth: 720 }}>
      <Body tone="muted" size={14}>Estimation de la semaine</Body>
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 28, color: palette.text, fontVariant: ['tabular-nums'] }}>{formatTotal(price.total_cents)}</Text>
      <Body tone="muted" size={14}>{`soit ${formatPrice(price.per_portion_cents)} la portion · ${price.portions} portions`}</Body>
      {price.budget_cents != null ? (
        <Body tone={price.over_budget ? 'alert' : 'muted'} size={14}>
          {price.over_budget === null ? `Plafond ${formatTotal(price.budget_cents)} : comparaison incomplète` : `${price.over_budget ? 'Au-dessus du' : 'Sous le'} plafond de ${formatTotal(price.budget_cents)}`}
        </Body>
      ) : null}
      <Stack gap={space[1]}>
        {price.meals.map((m, i) => row(`${shortDateText(m.date)} · ${splitTitle(m.title).main}`, pillText(m.cents, m.complete), `${m.date}-${m.meal}-${i}`))}
        {price.shared_cents ? row('Arrondis et articles partagés', formatTotal(price.shared_cents), 'shared') : null}
      </Stack>
      {price.missing ? <Body tone="muted" size={14}>{`${price.missing} article${price.missing > 1 ? 's' : ''} sans prix, hors du total`}</Body> : null}
      <Body tone="muted" size={13}>
        {`Prix moyens${price.updated_on ? `, mis à jour le ${shortDateText(price.updated_on)}` : ''} · placard compté au prorata · sources : Open Prices (ODbL), INSEE`}
      </Body>
    </Card>
  );
}
