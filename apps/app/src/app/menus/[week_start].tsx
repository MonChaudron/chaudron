import type { Meal, Menu, MenuSlot, Recipe, RecipeSummary, SlotInput } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { addDays, copySlots, dateText, dayLabel, searchKey, space, todayIso, weekDays } from '@chaudron/domain';
import { Redirect, Stack as Router, useLocalSearchParams, useRouter } from 'expo-router';
import { useMemo, useState } from 'react';
import { View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useLoad } from '../../lib/useLoad';
import { Back, Body, Button, Card, Choice, Columns, ErrorText, Field, ListRow, Loading, Pill, Row, Screen, Stack, Tag, Title } from '../../ui';
import { mealLabels, type MoveRequest, MovePanel, SlotSummary, TextAction, Warnings, warningsOf } from '../../ui/menu';

export default function MenuScreen() {
  const { week_start: weekStart, nouveau } = useLocalSearchParams<{ week_start: string; nouveau?: string }>();
  const { me } = useSession();
  const home = me?.households[0];
  if (me === null) return <Redirect href="/connexion" />;
  if (!home || !weekStart) {
    return (
      <Screen>
        <Loading />
      </Screen>
    );
  }
  return <MenuView householdId={home.id} weekStart={weekStart} isNew={nouveau === '1'} />;
}

/** Un plat en cours de composition. Les nombres vides suivent la présence des membres. */
interface Draft {
  key: string;
  date: string;
  meal: Meal;
  recipe: string;
  title: string;
  variant: string | null;
  covers: boolean;
  diners: string;
  boxes: string;
  note: string;
  portionsDate: string | null;
}

let drafts = 0;
const draftKey = () => `d${++drafts}`;

function toDraft(slot: MenuSlot): Draft {
  return {
    key: draftKey(),
    date: slot.date,
    meal: slot.meal,
    recipe: slot.recipe.slug,
    title: slot.recipe.title,
    variant: slot.variant.label,
    covers: slot.covers_next_lunch,
    diners: slot.diners_fixed ? String(slot.diners) : '',
    boxes: slot.next_lunch_covers_fixed ? String(slot.next_lunch_covers) : '',
    note: slot.note ?? '',
    portionsDate: slot.portions_date ?? null,
  };
}

function toInput(d: Draft): SlotInput {
  const count = (text: string) => (text.trim() === '' ? null : Number(text));
  return {
    date: d.date,
    meal: d.meal,
    recipe: d.recipe,
    variant: d.variant,
    covers_next_lunch: d.covers,
    diners: count(d.diners),
    next_lunch_covers: d.covers ? count(d.boxes) : null,
    portions_date: d.portionsDate,
    note: d.note.trim() || null,
  };
}

const isDate = (text: string) => /^\d{4}-\d{2}-\d{2}$/.test(text) && !Number.isNaN(Date.parse(`${text}T00:00:00Z`));

function MenuView({ householdId, weekStart, isNew }: { householdId: string; weekStart: string; isNew: boolean }) {
  const router = useRouter();
  const today = todayIso();
  const path = { id: householdId, week_start: weekStart };
  const menu = useLoad(async () => {
    const result = await api.GET('/api/v1/households/{id}/menus/{week_start}', { params: { path } });
    // Nouveau menu : rien à lire, on compose.
    if (isNew && result.response.status === 404) return { data: { week_start: weekStart, title: '', notes: '', slots: [], warnings: [] } as Menu };
    return result;
  }, [householdId, weekStart, isNew]);
  const [editing, setEditing] = useState(isNew);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const data = menu.data;
  if (!data) {
    return (
      <Screen>
        <Back href="/menus" label="Menus" />
        {menu.error ? <ErrorText>{menu.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }

  async function save(start: string, body: { title: string; notes: string; slots: SlotInput[] }) {
    setBusy(true);
    setError(null);
    try {
      const { data: saved, error: failure } = await api.PUT('/api/v1/households/{id}/menus/{week_start}', {
        params: { path: { id: householdId, week_start: start } },
        body,
      });
      if (failure || !saved) {
        setError(errorMessage(failure));
        return false;
      }
      if (start !== weekStart) router.replace(`/menus/${start}`);
      else {
        menu.setData(saved);
        setEditing(false);
        if (isNew) router.setParams({ nouveau: undefined });
      }
      return true;
    } catch (err) {
      setError(errorMessage(err));
      return false;
    } finally {
      setBusy(false);
    }
  }

  const title = data.title || `Menu du ${dateText(data.week_start)}`;
  return (
    <Screen>
      <Router.Screen options={{ title: `${title} · Chaudron` }} />
      <Back href="/menus" label="Menus" />
      {editing ? (
        <Composer householdId={householdId} menu={data} isNew={isNew} busy={busy} error={error} onSave={save} onCancel={() => (isNew ? router.replace('/menus') : setEditing(false))} />
      ) : (
        <Overview householdId={householdId} menu={data} today={today} busy={busy} error={error} setBusy={setBusy} setError={setError} onEdit={() => setEditing(true)} onChange={menu.setData} onCopy={(to) => save(to, { title: data.title, notes: data.notes, slots: copySlots(data.slots, data.week_start, to) })} />
      )}
    </Screen>
  );
}

/** Le menu publié : ses jours, ses plats, ses avertissements. */
function Overview({ householdId, menu, today, busy, error, setBusy, setError, onEdit, onChange, onCopy }: {
  householdId: string;
  menu: Menu;
  today: string;
  busy: boolean;
  error: string | null;
  setBusy: (b: boolean) => void;
  setError: (e: string | null) => void;
  onEdit: () => void;
  onChange: (menu: Menu) => void;
  onCopy: (to: string) => Promise<boolean>;
}) {
  const router = useRouter();
  const [panel, setPanel] = useState<string | null>(null);
  const [copyTo, setCopyTo] = useState(addDays(menu.slots.at(-1)?.date ?? menu.week_start, 1));
  const [confirm, setConfirm] = useState(false);
  const days = [...new Set(menu.slots.map((s) => s.date))];
  const loose = menu.warnings.filter((w) => !w.slots.length || !menu.slots.some((s) => w.slots.includes(s.id) && s.date === w.date));

  async function move(request: MoveRequest) {
    setBusy(true);
    setError(null);
    try {
      const { data, error: failure } = await api.POST('/api/v1/households/{id}/menus/{week_start}/move', {
        params: { path: { id: householdId, week_start: menu.week_start } },
        body: request,
      });
      if (data) {
        onChange(data);
        setPanel(null);
      } else setError(errorMessage(failure));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    setBusy(true);
    const { error: failure } = await api.DELETE('/api/v1/households/{id}/menus/{week_start}', {
      params: { path: { id: householdId, week_start: menu.week_start } },
    });
    setBusy(false);
    if (failure) setError(errorMessage(failure));
    else router.replace('/menus');
  }

  return (
    <>
      <Stack gap={space[3]}>
        <Title>{menu.title || `Menu du ${dateText(menu.week_start)}`}</Title>
        {menu.notes ? <Body tone="muted" style={{ maxWidth: 720 }}>{menu.notes}</Body> : null}
        <Row>
          <Button kind="primary" label="Modifier" onPress={onEdit} />
        </Row>
      </Stack>
      <ErrorText>{error}</ErrorText>
      <Warnings warnings={loose} />
      <Columns weights={[3, 2]}>
        <Stack gap={space[5]}>
          {days.length === 0 ? <Body tone="muted">Aucun plat.</Body> : null}
          {days.map((date) => (
            <Card key={date}>
              <Title level={2}>{dayLabel(date, today)}</Title>
              {menu.slots
                .filter((s) => s.date === date)
                .map((slot) => (
                  <View key={slot.id} style={{ gap: space[3] }}>
                    {menu.slots.some((s) => s.date === date && s.meal !== slot.meal) ? <Tag>{mealLabels[slot.meal]}</Tag> : null}
                    <SlotSummary
                      slot={slot}
                      warnings={warningsOf(menu.warnings, slot.id).filter((w) => w.date === slot.date)}
                      actions={panel === slot.id ? null : <TextAction label="Déplacer…" onPress={() => setPanel(slot.id)} />}
                    />
                    {panel === slot.id ? (
                      <MovePanel
                        slot={slot}
                        today={menu.week_start < today ? today : menu.week_start}
                        occupied={(d, meal) => menu.slots.filter((s) => s.date === d && s.meal === meal)}
                        onMove={(r) => void move(r)}
                        onCancel={() => setPanel(null)}
                        busy={busy}
                      />
                    ) : null}
                  </View>
                ))}
            </Card>
          ))}
        </Stack>
        <Stack gap={space[5]}>
          <Card>
            <Title level={2}>Refaire cette semaine</Title>
            <Body tone="muted" size={15}>Les mêmes plats, décalés au jour choisi ; les portions suivent la présence de ce jour-là.</Body>
            <Field label="Premier jour" value={copyTo} onChangeText={setCopyTo} placeholder="AAAA-MM-JJ" hint={isDate(copyTo) ? dayLabel(copyTo, today) : 'Date au format AAAA-MM-JJ'} />
            <Button label="Refaire" busy={busy} disabled={!isDate(copyTo) || copyTo === menu.week_start} onPress={() => void onCopy(copyTo)} />
          </Card>
          <Card>
            {confirm ? (
              <>
                <Body size={16}>Supprimer ce menu ? Les recettes restent.</Body>
                <Row>
                  <Button kind="danger" label="Supprimer" busy={busy} onPress={() => void remove()} />
                  <Button kind="quiet" label="Annuler" onPress={() => setConfirm(false)} />
                </Row>
              </>
            ) : (
              <Button kind="danger" label="Supprimer le menu" onPress={() => setConfirm(true)} />
            )}
          </Card>
        </Stack>
      </Columns>
    </>
  );
}

/** Composer ou modifier un menu : un jour après l'autre, des plats choisis parmi les recettes du foyer. */
function Composer({ householdId, menu, isNew, busy, error, onSave, onCancel }: {
  householdId: string;
  menu: Menu;
  isNew: boolean;
  busy: boolean;
  error: string | null;
  onSave: (start: string, body: { title: string; notes: string; slots: SlotInput[] }) => Promise<boolean>;
  onCancel: () => void;
}) {
  const today = todayIso();
  const [start, setStart] = useState(menu.week_start);
  const [title, setTitle] = useState(menu.title);
  const [notes, setNotes] = useState(menu.notes);
  const [slots, setSlots] = useState<Draft[]>(() => menu.slots.map(toDraft));
  const lastDay = slots.reduce((max, s) => (s.date > max ? s.date : max), addDays(start, 6));
  const [extra, setExtra] = useState(0);
  const span = Math.round((Date.parse(`${lastDay}T00:00:00Z`) - Date.parse(`${start}T00:00:00Z`)) / 86_400_000) + 1 + extra;
  const days = isDate(start) ? weekDays(start, Math.min(span, 28)) : [];
  const [picking, setPicking] = useState<string | null>(null);
  const recipes = useLoad(() => api.GET('/api/v1/households/{id}/recipes', { params: { path: { id: householdId } } }), [householdId]);
  const [full, setFull] = useState<Record<string, Recipe>>({});

  const loadRecipe = (slug: string) => {
    if (full[slug]) return;
    void api.GET('/api/v1/households/{id}/recipes/{slug}', { params: { path: { id: householdId, slug } } }).then(({ data }) => {
      if (data) setFull((f) => ({ ...f, [slug]: data }));
    });
  };
  const update = (key: string, change: Partial<Draft>) => setSlots((list) => list.map((s) => (s.key === key ? { ...s, ...change } : s)));
  const add = (date: string, meal: Meal, recipe: RecipeSummary) => {
    setSlots((list) => [
      ...list,
      { key: draftKey(), date, meal, recipe: recipe.slug, title: recipe.title, variant: null, covers: meal === 'dinner', diners: '', boxes: '', note: '', portionsDate: null },
    ]);
    setPicking(null);
  };
  // Un nouveau premier jour décale tout le menu.
  const moveStart = (next: string) => {
    setStart(next);
    if (isDate(next) && isDate(start)) {
      const shift = Math.round((Date.parse(`${next}T00:00:00Z`) - Date.parse(`${start}T00:00:00Z`)) / 86_400_000);
      setSlots((list) => list.map((s) => ({ ...s, date: addDays(s.date, shift), portionsDate: null })));
    }
  };
  const badCount = slots.some((s) => [s.diners, s.boxes].some((t) => t.trim() !== '' && !/^\d{1,2}$/.test(t.trim())));

  return (
    <>
      <Title>{isNew ? 'Nouveau menu' : 'Modifier le menu'}</Title>
      <Columns weights={[3, 2]}>
        <Stack gap={space[5]}>
          {days.map((date) => (
            <Card key={date}>
              <Title level={2}>{dayLabel(date, today)}</Title>
              {slots
                .filter((s) => s.date === date)
                .sort((a, b) => (a.meal === b.meal ? 0 : a.meal === 'lunch' ? -1 : 1))
                .map((slot) => (
                  <DishEditor key={slot.key} slot={slot} recipe={full[slot.recipe]} onOpen={() => loadRecipe(slot.recipe)} onChange={(c) => update(slot.key, c)} onRemove={() => setSlots((l) => l.filter((s) => s.key !== slot.key))} />
                ))}
              {picking === date ? null : <TextAction label="+ Ajouter un plat" onPress={() => setPicking(date)} />}
              {picking === date ? <Picker recipes={recipes.data} error={recipes.error} onPick={(meal, r) => add(date, meal, r)} onCancel={() => setPicking(null)} /> : null}
            </Card>
          ))}
          {span < 28 ? <Button kind="quiet" label="Ajouter un jour" onPress={() => setExtra(extra + 1)} /> : null}
        </Stack>
        <Stack gap={space[5]}>
          <Card>
            {isNew ? <Field label="Premier jour" value={start} onChangeText={moveStart} placeholder="AAAA-MM-JJ" hint={isDate(start) ? dayLabel(start, today) : 'Date au format AAAA-MM-JJ'} /> : null}
            <Field label="Titre" value={title} onChangeText={setTitle} placeholder="Trois express, deux séances" />
            <Field label="Notes" value={notes} onChangeText={setNotes} multiline placeholder="L’idée de la semaine, le jour des courses…" style={{ minHeight: 120, paddingVertical: space[3], textAlignVertical: 'top' }} />
          </Card>
          <ErrorText>{error}</ErrorText>
          {badCount ? <ErrorText>Les nombres de mangeurs et de boîtes vont de 0 à 99 (vide : selon la présence).</ErrorText> : null}
          <Row>
            <Button kind="primary" label="Enregistrer" busy={busy} disabled={!isDate(start) || badCount} onPress={() => void onSave(start, { title, notes, slots: slots.map(toInput) })} />
            <Button kind="quiet" label="Annuler" onPress={onCancel} />
          </Row>
        </Stack>
      </Columns>
    </>
  );
}

/** Un plat du menu : variante, boîtes du lendemain, nombres fixés, note. */
function DishEditor({ slot, recipe, onOpen, onChange, onRemove }: { slot: Draft; recipe?: Recipe; onOpen: () => void; onChange: (change: Partial<Draft>) => void; onRemove: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <View style={{ gap: space[3] }}>
      <Row style={{ justifyContent: 'space-between' }}>
        <View style={{ flex: 1, gap: space[1], minWidth: 0 }}>
          <Row gap={space[2]}>
            <Tag>{mealLabels[slot.meal]}</Tag>
            <Body size={17}>{slot.title}</Body>
          </Row>
          <Body tone="muted" size={15}>
            {[slot.variant ?? 'Variante par défaut', slot.covers ? 'boîtes du lendemain' : null, slot.diners ? `${slot.diners} au repas` : null, slot.boxes && slot.covers ? `${slot.boxes} boîtes` : null]
              .filter(Boolean)
              .join(' · ')}
          </Body>
        </View>
        <Button kind="quiet" label={open ? 'Fermer' : 'Régler'} onPress={() => { setOpen(!open); onOpen(); }} />
        <Button kind="quiet" label="Retirer" accessibilityLabel={`Retirer ${slot.title}`} onPress={onRemove} />
      </Row>
      {open ? (
        <Stack gap={space[3]}>
          {recipe && recipe.variants.length > 1 ? (
            <Choice label="Variante" value={slot.variant ?? recipe.variants[0].label} onChange={(v) => onChange({ variant: v })} options={recipe.variants.map((v) => ({ value: v.label, label: v.label }))} />
          ) : null}
          <Choice label="Repas" value={slot.meal} onChange={(meal) => onChange({ meal })} options={[{ value: 'lunch', label: 'Midi' }, { value: 'dinner', label: 'Soir' }]} />
          <Pill label="Fait les boîtes du lendemain midi" selected={slot.covers} onPress={() => onChange({ covers: !slot.covers })} />
          <Row gap={space[3]}>
            <View style={{ flex: 1, minWidth: 140 }}>
              <Field label="Au repas" value={slot.diners} onChangeText={(t) => onChange({ diners: t })} keyboardType="number-pad" placeholder="selon la présence" />
            </View>
            {slot.covers ? (
              <View style={{ flex: 1, minWidth: 140 }}>
                <Field label="Boîtes" value={slot.boxes} onChangeText={(t) => onChange({ boxes: t })} keyboardType="number-pad" placeholder="selon la présence" />
              </View>
            ) : null}
          </Row>
          <Field label="Note" value={slot.note} onChangeText={(t) => onChange({ note: t })} placeholder="riz à part, doubler la sauce…" />
        </Stack>
      ) : null}
    </View>
  );
}

/** Choisir une recette du foyer pour un jour. */
function Picker({ recipes, error, onPick, onCancel }: { recipes?: RecipeSummary[]; error: string | null; onPick: (meal: Meal, recipe: RecipeSummary) => void; onCancel: () => void }) {
  const [meal, setMeal] = useState<Meal>('dinner');
  const [query, setQuery] = useState('');
  const found = useMemo(() => {
    const words = searchKey(query).split(' ').filter(Boolean);
    return (recipes ?? []).filter((r) => words.every((w) => searchKey(r.title).includes(w))).slice(0, 8);
  }, [recipes, query]);
  return (
    <Stack gap={space[3]}>
      <Choice label="Repas" value={meal} onChange={setMeal} options={[{ value: 'lunch', label: 'Midi' }, { value: 'dinner', label: 'Soir' }]} />
      <Field label="Recette" value={query} onChangeText={setQuery} placeholder="curry, poulet…" autoFocus />
      {error ? <ErrorText>{error}</ErrorText> : !recipes ? <Loading /> : null}
      <View>
        {found.map((r, i) => (
          <ListRow key={r.slug} title={r.title} detail={r.default_variant.label} onPress={() => onPick(meal, r)} last={i === found.length - 1} />
        ))}
      </View>
      <Button kind="quiet" label="Annuler" onPress={onCancel} />
    </Stack>
  );
}
