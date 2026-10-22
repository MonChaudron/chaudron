import { type Aisle, errorMessage, type Ingredient, type IngredientState, type PortionRole, type RecipeInput, type Similar, type Unit } from '@chaudron/api';
import {
  aisles,
  equipment as equipmentLabels,
  fromFood,
  ingredientStates,
  label,
  portionRoles,
  recipeToInput,
  roleForAisle,
  searchKey,
  slugify,
  space,
  units,
} from '@chaudron/domain';
import { Redirect, Stack as Router, useLocalSearchParams, useRouter } from 'expo-router';
import { useEffect, useMemo, useState } from 'react';
import { Pressable, Text, View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useTheme } from '../../lib/theme';
import { useLoad } from '../../lib/useLoad';
import { Back, Body, Button, Card, Choice, Columns, ErrorText, Field, Loading, Pill, Row, Screen, Stack, Title, useWide } from '../../ui';
import { type Answer, sameButBetter, SimilarQuestion } from '../../ui/similar';

type Draft = RecipeInput & { slug: string };
type Line = Draft['ingredients'][number];
type Variant = Draft['variants'][number];
type Phase = Variant['phases'][number];

const unitOptions = (Object.keys(units) as Unit[]).map((value) => ({ value, label: units[value] }));
const roleOptions = (Object.keys(portionRoles) as PortionRole[]).map((value) => ({ value, label: portionRoles[value] }));
const stateOptions = (Object.keys(ingredientStates) as IngredientState[]).map((value) => ({ value, label: ingredientStates[value] }));
const aisleOptions = (Object.keys(aisles) as Aisle[]).map((value) => ({ value, label: aisles[value] }));
const monthLetters = ['J', 'F', 'M', 'A', 'M', 'J', 'J', 'A', 'S', 'O', 'N', 'D'];
const monthNames = ['janvier', 'février', 'mars', 'avril', 'mai', 'juin', 'juillet', 'août', 'septembre', 'octobre', 'novembre', 'décembre'];

const emptyPhase = (): Phase => ({ title: '', text: '', day_offset: 0, steps: [] });
const emptyVariant = (label: string, isDefault: boolean): Variant => ({
  label,
  equipment: [],
  active_min: 15,
  total_min: 30,
  vessels: 1,
  is_default: isDefault,
  phases: [emptyPhase()],
});
const emptyDraft = (): Draft => ({
  slug: '',
  title: '',
  description: '',
  tags: [],
  season_months: [],
  source: null,
  ingredients: [],
  variants: [emptyVariant('Plaque', true)],
});

/** Nombre saisi à la française (« 0,5 »), vide → nul. */
function parseNumber(text: string): number | null {
  const value = Number(text.replace(',', '.').trim());
  return text.trim() === '' || Number.isNaN(value) ? null : value;
}
const showNumber = (value: number | null | undefined) => (value == null ? '' : String(value).replace('.', ','));

/** Création (`/recettes/modifier`) ou modification (`/recettes/modifier?slug=…`) d'une recette. */
export default function EditRecipe() {
  const { slug } = useLocalSearchParams<{ slug?: string }>();
  const { me } = useSession();
  const home = me?.households[0];
  if (me === null) return <Redirect href="/connexion" />;
  if (!home) {
    return (
      <Screen>
        <Loading />
      </Screen>
    );
  }
  return <Loader householdId={home.id} slug={slug} />;
}

function Loader({ householdId, slug }: { householdId: string; slug?: string }) {
  const recipe = useLoad(
    () =>
      slug
        ? api.GET('/api/v1/households/{id}/recipes/{slug}', { params: { path: { id: householdId, slug } } })
        : Promise.resolve({ data: null }),
    [householdId, slug],
  );
  if (recipe.data === undefined) {
    return (
      <Screen>
        <Back href="/recettes" label="Recettes" />
        {recipe.error ? <ErrorText>{recipe.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }
  const initial: Draft = recipe.data ? (recipeToInput(recipe.data) as Draft) : emptyDraft();
  return <Editor householdId={householdId} initial={initial} editing={!!recipe.data} community={!!recipe.data?.community} />;
}

function Editor({ householdId, initial, editing, community }: { householdId: string; initial: Draft; editing: boolean; community: boolean }) {
  const router = useRouter();
  const [draft, setDraft] = useState<Draft>(initial);
  const [tags, setTags] = useState(initial.tags?.join(', ') ?? '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [message, setMessage] = useState('');
  // Recettes proches d'une recette nouvelle : la question est posée une fois, avant de publier.
  const [matches, setMatches] = useState<Similar[] | null>(null);
  const catalog = useLoad(() => api.GET('/api/v1/ingredients'), []);
  const equipment = useLoad(() => api.GET('/api/v1/equipment'), []);
  const update = (patch: Partial<Draft>) => setDraft({ ...draft, ...patch });
  const slug = editing ? draft.slug : slugify(draft.title);
  const names = useMemo(() => new Map((catalog.data ?? []).map((i) => [i.code, i])), [catalog.data]);

  const body = (): RecipeInput => ({
    ...draft,
    slug,
    tags: tags.split(',').map((t) => t.trim()).filter(Boolean),
    message: message.trim() || null,
  });

  const save = async () => {
    if (!editing && matches === null) {
      setBusy(true);
      const { data: close } = await api.POST('/api/v1/households/{id}/recipes/similar', { params: { path: { id: householdId } }, body: body() });
      setBusy(false);
      if (close?.length) return setMatches(close);
    }
    await publish();
  };

  const answer = async (choice: Answer, target: string) => {
    const close = matches?.find((m) => m.slug === target);
    if (choice === 'better' && close?.community) {
      setBusy(true);
      const result = await sameButBetter(householdId, target, slug, { ...body(), slug: undefined });
      setBusy(false);
      if (!result.slug) return setError(result.error ?? null);
      return router.replace(`/recettes/${result.slug}`);
    }
    if (choice === 'better') {
      setBusy(true);
      const { data, error: failure } = await api.PUT('/api/v1/households/{id}/recipes/{slug}', {
        params: { path: { id: householdId, slug: target } },
        body: { ...body(), slug: target, message: 'La même en mieux' },
      });
      setBusy(false);
      if (!data) return setError(errorMessage(failure));
      return router.replace(`/recettes/${data.slug}`);
    }
    await publish(choice === 'variant' ? target : undefined);
  };

  /** Publie ; `family` : rangée avec cette recette (une autre déclinaison). */
  const publish = async (family?: string) => {
    setBusy(true);
    const payload = body();
    // Une recette de la communauté : seul l'admin de l'instance arrive ici.
    const { data, error: failure } = community
      ? await api.PUT('/api/v1/community/recipes/{slug}', { params: { path: { slug } }, body: payload })
      : await api.PUT('/api/v1/households/{id}/recipes/{slug}', { params: { path: { id: householdId, slug } }, body: payload });
    if (data && family) {
      await api.PUT('/api/v1/households/{id}/recipes/{slug}/family', { params: { path: { id: householdId, slug } }, body: { with: family } });
    }
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    router.replace(`/recettes/${data.slug}`);
  };

  const remove = async () => {
    const { response, error: failure } = community
      ? await api.DELETE('/api/v1/community/recipes/{slug}', { params: { path: { slug } } })
      : await api.DELETE('/api/v1/households/{id}/recipes/{slug}', { params: { path: { id: householdId, slug } } });
    if (!response.ok) return setError(errorMessage(failure));
    router.replace('/recettes');
  };

  return (
    <Screen>
      <Router.Screen options={{ title: `${editing ? draft.title : 'Nouvelle recette'} · Chaudron` }} />
      <Back href={editing ? `/recettes/${slug}` : '/recettes'} label={editing ? draft.title : 'Recettes'} />
      <Title>{editing ? (community ? 'Modifier la recette de la communauté' : 'Modifier la recette') : 'Nouvelle recette'}</Title>

      <Columns weights={[2, 3]}>
        <Stack gap={space[5]}>
          <Card>
            <Field label="Titre" value={draft.title} onChangeText={(title) => update({ title })} maxLength={120} placeholder="Chili con carne" />
            {!editing && slug ? <Body tone="muted" size={14}>Adresse : /recettes/{slug}</Body> : null}
            <Field
              label="En une phrase"
              value={draft.description ?? ''}
              onChangeText={(description) => update({ description })}
              multiline
              style={{ minHeight: 96, paddingTop: 12, textAlignVertical: 'top' }}
              placeholder="Ce que c’est, pour qui, ce qui le rend bon."
            />
            <Field label="Étiquettes" hint="Séparées par des virgules : cookeo, express, végétarien." value={tags} onChangeText={setTags} autoCapitalize="none" />
            <Stack gap={space[2]}>
              <Body size={16}>De saison</Body>
              <Row gap={space[1]}>
                {monthLetters.map((letter, i) => {
                  const month = i + 1;
                  const on = draft.season_months?.includes(month) ?? false;
                  return (
                    <Pill
                      key={month}
                      compact
                      label={letter}
                      accessibilityLabel={monthNames[i]}
                      selected={on}
                      onPress={() =>
                        update({
                          season_months: on
                            ? (draft.season_months ?? []).filter((m) => m !== month)
                            : [...(draft.season_months ?? []), month].sort((a, b) => a - b),
                        })
                      }
                    />
                  );
                })}
              </Row>
            </Stack>
          </Card>

          <Ingredients
            lines={draft.ingredients}
            names={names}
            catalog={catalog.data ?? []}
            onChange={(ingredients) => update({ ingredients })}
            onCreated={catalog.reload}
          />
        </Stack>

        <Variants
          variants={draft.variants}
          catalog={(equipment.data ?? []).map((e) => ({ code: e.code, label: label(equipmentLabels, e.name_key) }))}
          onChange={(variants) => update({ variants })}
        />
      </Columns>

      <View style={{ gap: space[3], maxWidth: 480 }}>
        {editing ? (
          <Field
            label="Un mot sur ce changement"
            hint="Facultatif, gardé dans l’historique : « 14 min de vapeur, le cœur restait ferme »."
            value={message}
            onChangeText={setMessage}
            maxLength={300}
          />
        ) : null}
        {matches?.length ? <SimilarQuestion matches={matches} busy={busy} onAnswer={answer} /> : null}
        <ErrorText>{error}</ErrorText>
        <Button kind="primary" label={editing ? 'Enregistrer' : 'Publier la recette'} busy={busy} disabled={!draft.title.trim() || !slug} onPress={save} />
        {editing ? (
          confirmDelete ? (
            <Row>
              <Button kind="danger" label="Supprimer pour de bon" onPress={remove} />
              <Button kind="quiet" label="Garder" onPress={() => setConfirmDelete(false)} />
            </Row>
          ) : (
            <Button kind="quiet" label="Supprimer la recette" onPress={() => setConfirmDelete(true)} />
          )
        ) : null}
      </View>
    </Screen>
  );
}

// Ingrédients

function Ingredients({
  lines,
  names,
  catalog,
  onChange,
  onCreated,
}: {
  lines: Line[];
  names: Map<string, Ingredient>;
  catalog: Ingredient[];
  onChange: (lines: Line[]) => void;
  onCreated: () => Promise<unknown>;
}) {
  const [query, setQuery] = useState('');
  const [more, setMore] = useState(false);
  const needle = searchKey(query);
  const used = new Set(lines.map((l) => l.ingredient));
  const results = needle ? catalog.filter((i) => !used.has(i.code) && searchKey(i.name).includes(needle)).slice(0, 8) : [];

  const add = (ingredient: Ingredient) => {
    onChange([
      ...lines,
      {
        ingredient: ingredient.code,
        state: ingredient.default_state,
        qty: null,
        unit: ingredient.default_unit,
        role: roleForAisle(ingredient.aisle),
        optional: false,
        per_batch: false,
        note: null,
      },
    ]);
    setQuery('');
    setMore(false);
  };
  const set = (index: number, patch: Partial<Line>) => onChange(lines.map((l, i) => (i === index ? { ...l, ...patch } : l)));

  return (
    <Card>
      <Title level={2}>Ingrédients</Title>
      <Body tone="muted" size={15}>Pour une portion standard (un adulte), telles qu’achetées : le barème fait le reste.</Body>
      {lines.map((line, i) => (
        <LineEditor
          key={`${line.ingredient}-${i}`}
          line={line}
          name={names.get(line.ingredient)?.name ?? line.ingredient}
          onChange={(patch) => set(i, patch)}
          onRemove={() => onChange(lines.filter((_, j) => j !== i))}
        />
      ))}
      <Field label="Ajouter un ingrédient" placeholder="carot, lentilles…" value={query} onChangeText={setQuery} autoCorrect={false} />
      {results.length ? (
        <Row>
          {results.map((ingredient) => (
            <Pill key={ingredient.code} label={`+ ${ingredient.name}`} selected={false} onPress={() => add(ingredient)} />
          ))}
        </Row>
      ) : null}
      {needle && !more ? <Button kind="quiet" label="Plus de choix (table Ciqual)" onPress={() => setMore(true)} /> : null}
      {more && needle ? (
        <FoodSearch
          query={query}
          onCreated={async (ingredient) => {
            await onCreated();
            add(ingredient);
          }}
        />
      ) : null}
    </Card>
  );
}

function LineEditor({ line, name, onChange, onRemove }: { line: Line; name: string; onChange: (patch: Partial<Line>) => void; onRemove: () => void }) {
  const { palette, fonts, weights } = useTheme();
  const [qty, setQty] = useState(showNumber(line.qty));
  const [open, setOpen] = useState(false);
  return (
    <View style={{ gap: space[2], paddingBottom: space[3], borderBottomWidth: 1, borderBottomColor: palette.line }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: space[3] }}>
        <Text style={{ flex: 1, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 17, color: palette.text }}>{name}</Text>
        <View style={{ width: 88 }}>
          <Field
            label={`Quantité de ${name}`}
            hideLabel
            value={qty}
            placeholder="au goût"
            keyboardType="decimal-pad"
            onChangeText={(text) => {
              setQty(text);
              onChange({ qty: parseNumber(text) });
            }}
          />
        </View>
        <Text style={{ width: 52, fontFamily: fonts.text, fontSize: 16, color: palette.muted }}>{units[line.unit as Unit] ?? line.unit}</Text>
      </View>
      <Pressable onPress={() => setOpen(!open)} accessibilityRole="button" style={{ alignSelf: 'flex-start' }}>
        <Body tone="muted" size={15} style={{ textDecorationLine: 'underline' }}>
          {open
            ? 'Moins de réglages'
            : [portionRoles[line.role], ingredientStates[line.state ?? 'fresh'].toLowerCase(), line.optional ? 'facultatif' : null, line.per_batch ? 'pour le plat' : null, line.note]
                .filter(Boolean)
                .join(', ')}
        </Body>
      </Pressable>
      {open ? (
        <Stack>
          <Body size={15}>Unité</Body>
          <Choice label="Unité" options={unitOptions} value={line.unit} onChange={(unit) => onChange({ unit })} />
          <Body size={15}>Dans le barème</Body>
          <Choice label="Rôle" options={roleOptions} value={line.role} onChange={(role) => onChange({ role })} />
          <Body size={15}>Acheté</Body>
          <Choice label="État" options={stateOptions} value={line.state ?? 'fresh'} onChange={(state) => onChange({ state })} />
          <Row>
            <Pill label="Facultatif" selected={!!line.optional} onPress={() => onChange({ optional: !line.optional })} />
            <Pill label="Pour le plat entier" selected={!!line.per_batch} onPress={() => onChange({ per_batch: !line.per_batch })} />
          </Row>
          <Field label="Note" placeholder="ou colin ; sur la moitié ados…" value={line.note ?? ''} onChangeText={(note) => onChange({ note: note || null })} />
          <Button kind="danger" label="Retirer l’ingrédient" onPress={onRemove} />
        </Stack>
      ) : null}
    </View>
  );
}

/** « Plus de choix » : la table Ciqual ; choisir une ligne crée l'ingrédient, qui rejoint le référentiel. */
function FoodSearch({ query, onCreated }: { query: string; onCreated: (ingredient: Ingredient) => Promise<void> }) {
  const [search, setSearch] = useState(query);
  useEffect(() => {
    const timer = setTimeout(() => setSearch(query), 250);
    return () => clearTimeout(timer);
  }, [query]);
  const foods = useLoad(() => api.GET('/api/v1/foods', { params: { query: { q: search } } }), [search]);
  const [picked, setPicked] = useState<{ code: number; name: string } | null>(null);
  const [name, setName] = useState('');
  const [state, setState] = useState<IngredientState>('fresh');
  const [aisle, setAisle] = useState<Aisle>('produce');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pick = (food: { code: number; name: string }) => {
    const guess = fromFood(food.name);
    setPicked(food);
    setName(guess.name);
    setState(guess.state);
    setAisle(guess.state === 'frozen' ? 'frozen' : guess.state === 'canned' || guess.state === 'dried' ? 'grocery' : 'produce');
  };
  const create = async () => {
    if (!picked) return;
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/ingredients', {
      body: {
        name,
        aisle,
        default_unit: 'g',
        default_state: state,
        foods: [{ state, food_code: picked.code }],
      },
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    await onCreated(data);
  };

  if (picked) {
    return (
      <Stack>
        <Body size={15}>Nouvel ingrédient, d’après « {picked.name} »</Body>
        <Field label="Nom" value={name} onChangeText={setName} maxLength={80} />
        <Body size={15}>Acheté</Body>
        <Choice label="État" options={stateOptions} value={state} onChange={setState} />
        <Body size={15}>Rayon</Body>
        <Choice label="Rayon" options={aisleOptions} value={aisle} onChange={setAisle} />
        <ErrorText>{error}</ErrorText>
        <Row>
          <Button kind="primary" label="Créer et ajouter" busy={busy} disabled={!name.trim()} onPress={create} />
          <Button kind="quiet" label="Autre ligne" onPress={() => setPicked(null)} />
        </Row>
      </Stack>
    );
  }
  return (
    <Stack gap={space[1]}>
      {foods.data === undefined ? <Loading /> : null}
      {foods.data?.length === 0 ? <Body tone="muted" size={15}>Rien dans Ciqual pour « {query} ».</Body> : null}
      {foods.data?.map((food) => (
        <Pressable key={food.code} onPress={() => pick(food)} accessibilityRole="button" style={{ paddingVertical: space[2] }}>
          <Body size={16}>{food.name}</Body>
        </Pressable>
      ))}
    </Stack>
  );
}

// Variantes, phases, gestes

function Variants({ variants, catalog, onChange }: { variants: Variant[]; catalog: { code: string; label: string }[]; onChange: (variants: Variant[]) => void }) {
  const [current, setCurrent] = useState(0);
  const index = Math.min(current, variants.length - 1);
  const variant = variants[index];
  const set = (patch: Partial<Variant>) => onChange(variants.map((v, i) => (i === index ? { ...v, ...patch } : v)));

  return (
    <Card>
      <Title level={2}>Façons de cuisiner</Title>
      <Row>
        {variants.map((v, i) => (
          <Pill key={i} role="radio" label={v.label || `Façon ${i + 1}`} selected={i === index} onPress={() => setCurrent(i)} />
        ))}
        <Button
          kind="quiet"
          label="+ Une autre façon"
          onPress={() => {
            onChange([...variants, emptyVariant('', false)]);
            setCurrent(variants.length);
          }}
        />
      </Row>
      <Field label="Nom de la façon" placeholder="Cookeo, Four, Cocotte…" value={variant.label} onChangeText={(text) => set({ label: text })} maxLength={80} />
      <Stack gap={space[2]}>
        <Body size={16}>Avec</Body>
        <Row>
          {catalog.map((e) => {
            const on = variant.equipment?.includes(e.code) ?? false;
            return (
              <Pill
                key={e.code}
                label={e.label}
                selected={on}
                onPress={() => set({ equipment: on ? (variant.equipment ?? []).filter((c) => c !== e.code) : [...(variant.equipment ?? []), e.code] })}
              />
            );
          })}
        </Row>
      </Stack>
      <Row gap={space[3]}>
        <NumberField label="Minutes actives" value={variant.active_min} onChange={(active_min) => set({ active_min: active_min ?? 0 })} />
        <NumberField label="En tout" value={variant.total_min} onChange={(total_min) => set({ total_min: total_min ?? 0 })} />
        <NumberField label="Récipients" value={variant.vessels ?? 1} onChange={(vessels) => set({ vessels: vessels ?? 1 })} />
      </Row>
      {variants.length > 1 ? (
        <Row>
          <Pill
            label="Façon par défaut"
            selected={!!variant.is_default}
            onPress={() => onChange(variants.map((v, i) => ({ ...v, is_default: i === index })))}
          />
          <Button
            kind="danger"
            label="Retirer cette façon"
            onPress={() => {
              onChange(variants.filter((_, i) => i !== index));
              setCurrent(0);
            }}
          />
        </Row>
      ) : null}

      <Title level={2}>Déroulé</Title>
      <Body tone="muted" size={15}>Les phases, un écran chacune en cuisine ; les gestes détaillent une phase pour qui débute.</Body>
      {variant.phases.map((phase, i) => (
        <PhaseEditor
          key={`${index}-${i}`}
          number={i + 1}
          phase={phase}
          onChange={(patch) => set({ phases: variant.phases.map((p, j) => (j === i ? { ...p, ...patch } : p)) })}
          onRemove={variant.phases.length > 1 ? () => set({ phases: variant.phases.filter((_, j) => j !== i) }) : undefined}
        />
      ))}
      <Button label="Ajouter une phase" onPress={() => set({ phases: [...variant.phases, emptyPhase()] })} />
    </Card>
  );
}

function NumberField({ label: text, value, onChange }: { label: string; value: number | null | undefined; onChange: (value: number | null) => void }) {
  const [raw, setRaw] = useState(showNumber(value));
  return (
    <View style={{ width: 120 }}>
      <Field
        label={text}
        value={raw}
        keyboardType="number-pad"
        onChangeText={(t) => {
          setRaw(t);
          onChange(parseNumber(t));
        }}
      />
    </View>
  );
}

function PhaseEditor({ number, phase, onChange, onRemove }: { number: number; phase: Phase; onChange: (patch: Partial<Phase>) => void; onRemove?: () => void }) {
  const { palette, fonts, weights } = useTheme();
  const wide = useWide();
  const steps = phase.steps ?? [];
  return (
    <View style={{ gap: space[3], paddingTop: space[3], borderTopWidth: 1, borderTopColor: palette.line }}>
      <View style={{ flexDirection: 'row', gap: space[3], alignItems: 'flex-start' }}>
        <Text style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: 28, color: palette.link, width: 32 }}>{number}</Text>
        <View style={{ flex: 1, gap: space[3] }}>
          <Field label="Titre" placeholder="La farce" value={phase.title} onChangeText={(title) => onChange({ title })} maxLength={120} />
          <Field
            label="Ce qu’on fait"
            placeholder="Oignon et bœuf 5 min en mode dorer, écrasés à la spatule."
            value={phase.text}
            onChangeText={(text) => onChange({ text })}
            multiline
            style={{ minHeight: 96, paddingTop: 12, textAlignVertical: 'top' }}
          />
          <View style={wide ? { flexDirection: 'row', gap: space[3], alignItems: 'flex-end' } : { gap: space[3], alignItems: 'flex-start' }}>
            <NumberField
              label="Minuteur (min)"
              value={phase.timer_s ? phase.timer_s / 60 : null}
              onChange={(min) => onChange({ timer_s: min ? Math.round(min * 60) : null })}
            />
            <Pill label="La veille" selected={(phase.day_offset ?? 0) < 0} onPress={() => onChange({ day_offset: (phase.day_offset ?? 0) < 0 ? 0 : -1 })} />
          </View>
          {steps.map((step, j) => (
            <View key={j} style={{ flexDirection: 'row', gap: space[2], alignItems: 'flex-end' }}>
              <View style={{ flex: 1 }}>
                <Field
                  label={`Geste ${j + 1}`}
                  value={step.text}
                  onChangeText={(text) => onChange({ steps: steps.map((s, k) => (k === j ? { ...s, text } : s)) })}
                  multiline
                />
              </View>
              <Button kind="quiet" label="Retirer" onPress={() => onChange({ steps: steps.filter((_, k) => k !== j) })} />
            </View>
          ))}
          <Row>
            <Button kind="quiet" label="+ Un geste (pas à pas)" onPress={() => onChange({ steps: [...steps, { text: '' }] })} />
            {onRemove ? <Button kind="quiet" label="Retirer la phase" onPress={onRemove} /> : null}
          </Row>
        </View>
      </View>
    </View>
  );
}
