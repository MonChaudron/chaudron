// Remplacements d'ingrédients : sur la fiche (ce que la recette accepte, précise ou refuse) et dans « À revoir »
// (les saisir, les ajuster, valider une variante tentée).
import type { Feedback, Ingredient, Recipe, Substitution } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { searchKey, space } from '@chaudron/domain';
import { useState } from 'react';
import { View } from 'react-native';

import { api } from '../lib/api';
import { useLoad } from '../lib/useLoad';
import { Body, Button, Card, ErrorText, Field, Pill, Row, Title } from './index';

const factorText = (f: number) => (Math.abs(f - 1) < 1e-3 ? 'même quantité' : `quantité × ${String(Math.round(f * 100) / 100).replace('.', ',')}`);

/** Les remplacements des ingrédients de la recette ; ceux qui la modifient les précisent ou les refusent. */
export function RecipeSubstitutions({ householdId, recipe, editable, onChange }: { householdId: string; recipe: Recipe; editable: boolean; onChange: () => void }) {
  const all = useLoad(() => api.GET('/api/v1/substitutions'), []);
  const [error, setError] = useState<string | null>(null);
  const codes = new Set(recipe.ingredients.map((l) => l.ingredient));
  const mine = (all.data ?? []).filter((s) => codes.has(s.ingredient));
  if (!mine.length) return null;
  const rule = (s: Substitution) => recipe.substitutions.find((r) => r.substitution_id === s.id);
  const set = async (s: Substitution, allowed: boolean | null) => {
    const path = { params: { path: { id: householdId, slug: recipe.slug, substitution_id: s.id } } };
    const { response, error: failure } =
      allowed === null ? await api.DELETE('/api/v1/households/{id}/recipes/{slug}/substitutions/{substitution_id}', path) : await api.PUT('/api/v1/households/{id}/recipes/{slug}/substitutions/{substitution_id}', { ...path, body: { allowed, note: rule(s)?.note ?? '' } });
    if (!response.ok) return setError(errorMessage(failure));
    onChange();
  };
  return (
    <Card>
      <Title level={2}>Remplacements</Title>
      <Body tone="muted" size={15}>Proposés sur la liste de courses, devant un rayon vide ou une promo.</Body>
      {mine.map((s) => {
        const r = rule(s);
        const refused = r ? !r.allowed : false;
        return (
          <View key={s.id} style={{ gap: space[1] }}>
            <Body size={16} style={refused ? { textDecorationLine: 'line-through' } : undefined}>
              {`${s.substitute_name} pour ${s.ingredient_name.toLowerCase()}, ${factorText(s.factor)}${r?.note || s.note ? ` : ${r?.note || s.note}` : ''}`}
            </Body>
            {editable ? (
              <Row>
                <Pill compact label={refused ? 'Refusé dans cette recette' : 'Pas dans cette recette'} selected={refused} onPress={() => set(s, refused ? null : false)} />
              </Row>
            ) : null}
          </View>
        );
      })}
      <ErrorText>{error}</ErrorText>
    </Card>
  );
}

/** Choisir un ingrédient du référentiel par son nom. */
function IngredientPicker({ label, catalog, value, onChange }: { label: string; catalog: Ingredient[]; value: Ingredient | null; onChange: (i: Ingredient | null) => void }) {
  const [query, setQuery] = useState('');
  const needle = searchKey(query);
  const matches = needle ? catalog.filter((i) => searchKey(i.name).includes(needle)).slice(0, 6) : [];
  return (
    <View style={{ gap: space[2] }}>
      <Field label={label} value={value ? value.name : query} onChangeText={(t) => { onChange(null); setQuery(t); }} autoCorrect={false} />
      {!value && matches.length ? (
        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
          {matches.map((i) => (
            <Pill key={i.code} compact label={i.name} selected={false} onPress={() => onChange(i)} />
          ))}
        </View>
      ) : null}
    </View>
  );
}

/** « À revoir › Remplacements » : la liste de l'instance, en saisir un, en retirer un. */
export function SubstitutionsAdmin() {
  const all = useLoad(() => api.GET('/api/v1/substitutions'), []);
  const catalog = useLoad(() => api.GET('/api/v1/ingredients'), []);
  const [from, setFrom] = useState<Ingredient | null>(null);
  const [to, setTo] = useState<Ingredient | null>(null);
  const [factor, setFactor] = useState('1');
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const add = async () => {
    if (!from || !to) return;
    const { data, error: failure } = await api.POST('/api/v1/community/substitutions', {
      body: { ingredient: from.code, substitute: to.code, factor: Number(factor.replace(',', '.')), note },
    });
    if (!data) return setError(errorMessage(failure));
    setFrom(null);
    setTo(null);
    setFactor('1');
    setNote('');
    void all.reload();
  };
  const remove = async (s: Substitution) => {
    const { response, error: failure } = await api.DELETE('/api/v1/community/substitutions/{substitution_id}', { params: { path: { substitution_id: s.id } } });
    if (!response.ok) return setError(errorMessage(failure));
    void all.reload();
  };
  return (
    <Card style={{ maxWidth: 820 }}>
      <Title level={2}>{`Remplacements (${all.data?.length ?? 0})`}</Title>
      {(all.data ?? []).map((s) => (
        <Row key={s.id} gap={space[3]}>
          <Body size={16} style={{ flex: 1 }}>{`${s.substitute_name} pour ${s.ingredient_name.toLowerCase()}, ${factorText(s.factor)}${s.note ? ` : ${s.note}` : ''}${s.source === 'feedback' ? ' (variante tentée)' : ''}`}</Body>
          <Button kind="quiet" label="Retirer" onPress={() => remove(s)} />
        </Row>
      ))}
      <IngredientPicker label="Ingrédient remplacé" catalog={catalog.data ?? []} value={from} onChange={setFrom} />
      <IngredientPicker label="Remplaçant" catalog={catalog.data ?? []} value={to} onChange={setTo} />
      <Row gap={space[2]} style={{ alignItems: 'flex-end' }}>
        <View style={{ width: 140 }}>
          <Field label="Quantité ×" value={factor} onChangeText={setFactor} keyboardType="decimal-pad" />
        </View>
        <View style={{ flex: 1, minWidth: 200 }}>
          <Field label="Note" value={note} onChangeText={setNote} maxLength={300} placeholder="aussi bon, un peu plus ferme" />
        </View>
      </Row>
      <ErrorText>{error}</ErrorText>
      <View style={{ alignSelf: 'flex-start' }}>
        <Button kind="primary" label="Ajouter" disabled={!from || !to || !(Number(factor.replace(',', '.')) > 0)} onPress={add} />
      </View>
    </Card>
  );
}

/** Une variante tentée, dans « À revoir » : la faire entrer dans les remplacements, avec le coefficient. */
export function TriedToSubstitution({ feedback, onDone }: { feedback: Feedback; onDone: () => void }) {
  const tried = feedback.tried as { replaced?: string; by?: string; verdict?: string } | null;
  const [factor, setFactor] = useState('1');
  const [error, setError] = useState<string | null>(null);
  if (!tried?.replaced || !tried.by || tried.verdict !== 'good') return null;
  const add = async () => {
    const { data, error: failure } = await api.POST('/api/v1/community/substitutions', {
      body: { ingredient: tried.replaced ?? '', substitute: tried.by ?? '', factor: Number(factor.replace(',', '.')), feedback_id: feedback.id },
    });
    if (!data) return setError(errorMessage(failure));
    onDone();
  };
  return (
    <Row gap={space[2]} style={{ alignItems: 'flex-end' }}>
      <View style={{ width: 140 }}>
        <Field label="Quantité ×" value={factor} onChangeText={setFactor} keyboardType="decimal-pad" />
      </View>
      <Button kind="primary" label="Ajouter aux remplacements" onPress={add} />
      <ErrorText>{error}</ErrorText>
    </Row>
  );
}
