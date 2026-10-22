// « Ressemble à Gratin dauphinois : c'en est une version ? » Trois réponses : non, une autre déclinaison, la même en
// mieux. Rien n'est automatique : sans réponse, la recette reste à part.
import type { Recipe, Similar } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { space } from '@chaudron/domain';
import { Link } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';

import { api } from '../lib/api';
import { Body, Button, Card, ErrorText, Pill, Row, Title } from './index';

export type Answer = 'apart' | 'variant' | 'better';

export function SimilarQuestion({ matches, busy, better = true, onAnswer }: { matches: Similar[]; busy?: boolean; better?: boolean; onAnswer: (answer: Answer, slug: string) => void }) {
  const [chosen, setChosen] = useState(matches[0]?.slug ?? '');
  const match = matches.find((m) => m.slug === chosen) ?? matches[0];
  if (!match) return null;
  return (
    <Card>
      <Title level={2}>{`Ressemble à « ${match.title} » : c’en est une version ?`}</Title>
      {matches.length > 1 ? (
        <View accessibilityRole="radiogroup" style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
          {matches.map((m) => (
            <Pill key={m.slug} compact role="radio" label={`${m.title}${m.community ? ' (communauté)' : ''}`} selected={m.slug === chosen} onPress={() => setChosen(m.slug)} />
          ))}
        </View>
      ) : null}
      <Body tone="muted" size={15}>
        {[...match.reasons, `ressemblance ${match.score} %`, match.community ? 'recette de la communauté' : 'recette du foyer'].join(' · ')}{' '}
        <Link href={`/recettes/${match.slug}`} style={{ textDecorationLine: 'underline' }}>
          la voir
        </Link>
      </Body>
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
        <Button label="Non, une recette à part" busy={busy} onPress={() => onAnswer('apart', match.slug)} />
        <Button label="Oui, une autre déclinaison" busy={busy} onPress={() => onAnswer('variant', match.slug)} />
        {better ? <Button label="Oui, la même en mieux" busy={busy} onPress={() => onAnswer('better', match.slug)} /> : null}
      </View>
      {better ? (
        <Body tone="muted" size={14}>
          {match.community
            ? 'La même en mieux : elle devient ta version de la recette de la communauté, et ses différences partent en propositions.'
            : 'La même en mieux : elle remplace l’existante, qui garde son historique.'}
        </Body>
      ) : null}
    </Card>
  );
}

/**
 * « La même en mieux » d'une recette de la communauté : le brouillon devient la copie du foyer (rattachée à
 * l'originale), et ses différences partent en propositions. Renvoie le slug de la copie.
 */
export async function sameButBetter(householdId: string, target: string, slug: string, draft: unknown): Promise<{ slug?: string; error?: string }> {
  const copy = await api.POST('/api/v1/households/{id}/recipes/{slug}/copy', {
    params: { path: { id: householdId, slug: target } },
    body: { slug, recipe: draft as never },
  });
  if (!copy.data) return { error: errorMessage(copy.error) };
  await api.POST('/api/v1/households/{id}/recipes/{slug}/propose-changes', { params: { path: { id: householdId, slug: copy.data.slug } }, body: {} });
  return { slug: copy.data.slug };
}

/** Les déclinaisons d'une recette : la préférée du foyer, et en sortir. */
export function FamilyCard({ householdId, recipe, editable, onChange }: { householdId: string; recipe: Recipe; editable: boolean; onChange: () => void }) {
  const [error, setError] = useState<string | null>(null);
  const family = recipe.family;
  if (!family) return null;
  const prefer = async () => {
    const { response, error: failure } = await api.PUT('/api/v1/households/{id}/families/{family_id}/preferred', {
      params: { path: { id: householdId, family_id: family.id } },
      body: { slug: recipe.slug },
    });
    if (!response.ok) return setError(errorMessage(failure));
    onChange();
  };
  const leave = async () => {
    const { response, error: failure } = await api.DELETE('/api/v1/households/{id}/recipes/{slug}/family', { params: { path: { id: householdId, slug: recipe.slug } } });
    if (!response.ok) return setError(errorMessage(failure));
    onChange();
  };
  return (
    <Card>
      <Title level={2}>{`Les déclinaisons de « ${family.title} »`}</Title>
      <View style={{ gap: space[1] }}>
        {family.members.map((m) => (
          <Row key={m.slug} gap={space[2]}>
            {m.slug === recipe.slug ? (
              <Body size={16} style={{ fontWeight: '600' }}>{m.title}</Body>
            ) : (
              <Link href={`/recettes/${m.slug}`} style={{ textDecorationLine: 'underline' }}>
                <Body size={16}>{m.title}</Body>
              </Link>
            )}
            {family.preferred === m.slug ? <Body tone="muted" size={14}>la préférée chez vous</Body> : null}
            {m.community ? <Body tone="muted" size={14}>communauté</Body> : null}
          </Row>
        ))}
      </View>
      <ErrorText>{error}</ErrorText>
      <Row>
        {family.preferred !== recipe.slug ? <Button label="Notre préférée" onPress={prefer} /> : null}
        {editable ? <Button kind="quiet" label="Ce n’est pas une déclinaison" onPress={leave} /> : null}
      </Row>
    </Card>
  );
}
