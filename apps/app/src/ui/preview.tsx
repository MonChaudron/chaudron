// Une recette en lecture seule, pour une portion standard : ce que l'admin voit d'une recette proposée.
import type { Recipe } from '@chaudron/api';
import { formatDuration, formatQuantity, space } from '@chaudron/domain';
import { View } from 'react-native';

import { Body, Card, Columns, Title } from './index';

export function RecipePreview({ recipe }: { recipe: Recipe }) {
  return (
    <Columns weights={[2, 3]}>
      <Card>
        <Title level={2}>Ingrédients</Title>
        <Body tone="muted" size={14}>Pour une portion standard.</Body>
        {recipe.ingredients.map((l) => (
          <Body key={l.key} size={16}>
            {`${l.name} : ${formatQuantity(l.qty ?? null, l.unit)}${l.note ? ` (${l.note})` : ''}`}
          </Body>
        ))}
      </Card>
      <View style={{ gap: space[4] }}>
        {recipe.variants.map((v) => (
          <Card key={v.key}>
            <Title level={2}>{v.label}</Title>
            <Body tone="muted" size={14}>{`${formatDuration(v.active_min)} actives, ${formatDuration(v.total_min)} en tout`}</Body>
            {v.phases.map((p, i) => (
              <View key={p.key} style={{ gap: 2 }}>
                <Body size={16} style={{ fontWeight: '600' }}>{`${i + 1}. ${p.title}${p.timer_s ? ` (${formatDuration(Math.max(1, Math.round(p.timer_s / 60)))})` : ''}`}</Body>
                <Body size={15}>{p.text}</Body>
              </View>
            ))}
          </Card>
        ))}
      </View>
    </Columns>
  );
}
