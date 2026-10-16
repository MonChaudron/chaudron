import { type Diet, errorMessage, type ExclusionKind, type Member, type MemberInput, type MemberKind, type Portions } from '@chaudron/api';
import {
  diets,
  exclusionKinds,
  formatFactor,
  ingredients as ingredientLabels,
  label,
  meals,
  memberKinds,
  portionRoles,
  searchKey,
  space,
  weekdays,
  weekdaysLetter,
} from '@chaudron/domain';
import { Stack as Router, useLocalSearchParams, useRouter } from 'expo-router';
import { useMemo, useState } from 'react';
import { Pressable, Text, View } from 'react-native';

import { api } from '../../../lib/api';
import { useSession } from '../../../lib/session';
import { useTheme } from '../../../lib/theme';
import { useLoad } from '../../../lib/useLoad';
import { Back, Body, Button, Card, Choice, Columns, ErrorText, Field, Loading, Pill, Row, Screen, Stack, Title, useWide } from '../../../ui';

type Role = keyof typeof portionRoles;
const kindOptions = (Object.keys(memberKinds) as MemberKind[]).map((value) => ({ value, label: memberKinds[value] }));
const dietOptions = (Object.keys(diets) as Diet[]).map((value) => ({ value, label: diets[value] }));
const STEP = 0.1;

const toInput = (m: Member): MemberInput => ({
  name: m.name,
  kind: m.kind,
  diet: m.diet,
  user_id: m.user_id ?? null,
  portions: m.portions,
  presence: m.presence,
  exclusions: m.exclusions,
});

export default function MemberScreen() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { me } = useSession();
  const householdId = me?.households[0]?.id;
  if (!householdId) {
    return (
      <Screen>
        <Loading />
      </Screen>
    );
  }
  return <MemberForm householdId={householdId} memberId={id} />;
}

function MemberForm({ householdId, memberId }: { householdId: string; memberId: string }) {
  const router = useRouter();
  const { weights } = useTheme();
  const wide = useWide();
  const path = { params: { path: { id: householdId, member_id: memberId } } };
  const member = useLoad(() => api.GET('/api/v1/households/{id}/members/{member_id}', path), [householdId, memberId]);
  const household = useLoad(() => api.GET('/api/v1/households/{id}', { params: { path: { id: householdId } } }), [householdId]);
  const others = useLoad(() => api.GET('/api/v1/households/{id}/members', { params: { path: { id: householdId } } }), [householdId]);
  const defaults = useLoad(() => api.GET('/api/v1/portions/defaults'), []);
  const [draft, setDraft] = useState<MemberInput | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const saved = member.data ? toInput(member.data) : null;
  const form = draft ?? saved;

  if (!form) {
    return (
      <Screen>
        <Back href="/foyer" label="Le foyer" />
        {member.error ? <ErrorText>{member.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }

  const dirty = draft !== null && JSON.stringify(draft) !== JSON.stringify(saved);
  const update = (patch: Partial<MemberInput>) => setDraft({ ...form, ...patch });
  const save = async () => {
    setBusy(true);
    const { data, error: failure } = await api.PUT('/api/v1/households/{id}/members/{member_id}', { ...path, body: form });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    setError(null);
    member.setData(data);
    setDraft(null);
  };
  const remove = async () => {
    const { response, error: failure } = await api.DELETE('/api/v1/households/{id}/members/{member_id}', path);
    if (!response.ok) return setError(errorMessage(failure));
    router.replace('/foyer');
  };

  // Comptes du foyer pas encore associés à un autre membre.
  const taken = new Set((others.data ?? []).filter((m) => m.id !== memberId).map((m) => m.user_id));
  const accounts = (household.data?.users ?? []).filter((u) => !taken.has(u.user_id));
  const kindDefaults = defaults.data?.[form.kind];

  return (
    <Screen>
      <Router.Screen options={{ title: `${form.name || 'Membre'} · Chaudron` }} />
      <Back href="/foyer" label="Le foyer" />
      <Title>{form.name || 'Sans nom'}</Title>

      <Columns>
        <>
          <Card>
            <Field label="Prénom" value={form.name} onChangeText={(name) => update({ name })} maxLength={80} />
            <Stack gap={space[2]}>
              <Body size={15} style={{ fontWeight: weights.medium }}>Type</Body>
              <Choice label="Type" options={kindOptions} value={form.kind} onChange={(kind) => update({ kind })} />
            </Stack>
            <Stack gap={space[2]}>
              <Body size={15} style={{ fontWeight: weights.medium }}>Régime</Body>
              <Choice label="Régime" options={dietOptions} value={form.diet} onChange={(diet) => update({ diet })} />
            </Stack>
            {accounts.length ? (
              <Stack gap={space[2]}>
                <Body size={15} style={{ fontWeight: weights.medium }}>Son compte</Body>
                <Choice
                  label="Son compte"
                  options={[{ value: '', label: 'Pas de compte' }, ...accounts.map((u) => ({ value: u.user_id, label: u.display_name }))]}
                  value={form.user_id ?? ''}
                  onChange={(user) => update({ user_id: user || null })}
                />
              </Stack>
            ) : null}
          </Card>

          <Card>
            <Title level={2}>Portions</Title>
            <Body tone="muted" size={16}>Combien de portions standard pour chaque famille d’ingrédients : ×2 de féculents pour un ado qui a faim.</Body>
            <PortionsEditor portions={form.portions} onChange={(portions) => update({ portions })} />
            {kindDefaults && JSON.stringify(roundAll(kindDefaults)) !== JSON.stringify(roundAll(form.portions)) ? (
              <Button kind="quiet" label={`Remettre le barème ${memberKinds[form.kind].toLowerCase()}`} onPress={() => update({ portions: kindDefaults })} />
            ) : null}
          </Card>
        </>
        <>
          <Card>
            <Title level={2}>Présence</Title>
            <Body tone="muted" size={16}>Les repas où {form.name || 'cette personne'} mange à la maison.</Body>
            <PresenceGrid presence={form.presence} onChange={(presence) => update({ presence })} />
          </Card>

          <Card>
            <Title level={2}>À éviter</Title>
            <Body tone="muted" size={16}>Une allergie écarte toujours l’ingrédient ; « n’aime pas » le signale seulement.</Body>
            <Exclusions value={form.exclusions} onChange={(exclusions) => update({ exclusions })} />
          </Card>
        </>
      </Columns>

      <View style={wide ? { alignSelf: 'flex-end', width: 360, gap: space[3] } : { gap: space[3] }}>
        <ErrorText>{error}</ErrorText>
        <Button kind="primary" label={dirty ? 'Enregistrer' : 'Enregistré'} busy={busy} disabled={!dirty || !form.name.trim()} onPress={save} />
        {confirmDelete ? (
          <Row>
            <View style={{ flex: 1 }}>
              <Button kind="danger" label={`Oui, retirer ${form.name}`} onPress={remove} />
            </View>
            <Button kind="quiet" label="Annuler" onPress={() => setConfirmDelete(false)} />
          </Row>
        ) : (
          <Button kind="quiet" label="Retirer ce membre du foyer" onPress={() => setConfirmDelete(true)} />
        )}
      </View>
    </Screen>
  );
}

const round = (value: number) => Math.round(value * 10) / 10;
const roundAll = (p: Portions) => Object.fromEntries(Object.entries(p).map(([k, v]) => [k, round(v)]));

function PortionsEditor({ portions, onChange }: { portions: Portions; onChange: (p: Portions) => void }) {
  const { palette, fonts, weights } = useTheme();
  const roles = Object.keys(portionRoles) as Role[];
  const set = (role: Role, value: number) => onChange({ ...portions, [role]: Math.min(5, Math.max(0, round(value))) });
  return (
    <View>
      {roles.map((role, index) => (
        <View
          key={role}
          style={{ flexDirection: 'row', alignItems: 'center', gap: space[3], paddingVertical: space[2], borderBottomWidth: index < roles.length - 1 ? 1 : 0, borderBottomColor: palette.line }}
        >
          <Body style={{ flex: 1 }}>{portionRoles[role]}</Body>
          <Step label="−" accessibilityLabel={`Moins de ${portionRoles[role].toLowerCase()}`} onPress={() => set(role, portions[role] - STEP)} disabled={portions[role] <= 0} />
          <Text style={{ width: 56, textAlign: 'center', fontFamily: fonts.title, fontWeight: weights.title, fontSize: 18, color: palette.text }}>
            {formatFactor(portions[role])}
          </Text>
          <Step label="+" accessibilityLabel={`Plus de ${portionRoles[role].toLowerCase()}`} onPress={() => set(role, portions[role] + STEP)} disabled={portions[role] >= 5} />
        </View>
      ))}
    </View>
  );
}

function Step({ label: text, onPress, disabled, accessibilityLabel }: { label: string; onPress: () => void; disabled: boolean; accessibilityLabel: string }) {
  const { palette, fonts, radius, weights } = useTheme();
  return (
    <Pressable
      onPress={onPress}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      style={({ pressed }) => ({
        width: 44,
        height: 44,
        borderRadius: radius.tag,
        borderWidth: 1.5,
        borderColor: palette.line,
        alignItems: 'center',
        justifyContent: 'center',
        opacity: disabled ? 0.4 : pressed ? 0.7 : 1,
      })}
    >
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 20, color: palette.text }}>{text}</Text>
    </Pressable>
  );
}

function PresenceGrid({ presence, onChange }: { presence: MemberInput['presence']; onChange: (p: MemberInput['presence']) => void }) {
  const { weights } = useTheme();
  const toggle = (meal: 'lunch' | 'dinner', day: number) =>
    onChange({ ...presence, [meal]: presence[meal].map((here, i) => (i === day ? !here : here)) });
  return (
    <Stack gap={space[3]}>
      {(['lunch', 'dinner'] as const).map((meal) => (
        <View key={meal} style={{ gap: space[2] }}>
          <Body size={15} style={{ fontWeight: weights.medium }}>{meals[meal]}</Body>
          <View style={{ flexDirection: 'row', justifyContent: 'space-between', gap: 4 }}>
            {weekdaysLetter.map((letter, day) => (
              <Pill
                key={day}
                compact
                label={letter}
                accessibilityLabel={`${meals[meal]}, ${weekdays[day]}`}
                selected={presence[meal][day]}
                onPress={() => toggle(meal, day)}
              />
            ))}
          </View>
        </View>
      ))}
    </Stack>
  );
}

function Exclusions({ value, onChange }: { value: MemberInput['exclusions']; onChange: (e: MemberInput['exclusions']) => void }) {
  const catalog = useLoad(() => api.GET('/api/v1/ingredients'), []);
  const [query, setQuery] = useState('');
  const [kind, setKind] = useState<ExclusionKind>('allergy');
  const names = useMemo(
    () => (catalog.data ?? []).map((i) => ({ id: i.id, name: label(ingredientLabels, i.name_key) })),
    [catalog.data],
  );
  const nameOf = (id: string) => names.find((n) => n.id === id)?.name ?? '…';
  const excluded = new Set(value.map((e) => e.ingredient_id));
  const needle = searchKey(query);
  const results = needle
    ? names.filter((n) => !excluded.has(n.id) && searchKey(n.name).includes(needle)).slice(0, 6)
    : [];

  return (
    <Stack>
      {value.length ? (
        <Row>
          {value.map((e) => (
            <Pill
              key={e.ingredient_id}
              selected
              tone={e.kind === 'allergy' ? 'alert' : 'normal'}
              label={`${nameOf(e.ingredient_id)} · ${exclusionKinds[e.kind].toLowerCase()}  ×`}
              accessibilityLabel={`Retirer ${nameOf(e.ingredient_id)}`}
              onPress={() => onChange(value.filter((x) => x.ingredient_id !== e.ingredient_id))}
            />
          ))}
        </Row>
      ) : (
        <Body tone="muted" size={16}>Rien à éviter.</Body>
      )}
      <Choice
        label="Type d’exclusion"
        options={[
          { value: 'allergy', label: exclusionKinds.allergy },
          { value: 'dislike', label: exclusionKinds.dislike },
        ]}
        value={kind}
        onChange={setKind}
      />
      <Field label="Ajouter un ingrédient" placeholder="Arachide, champignon…" value={query} onChangeText={setQuery} autoCorrect={false} />
      {results.length ? (
        <Row>
          {results.map((r) => (
            <Pill
              key={r.id}
              selected={false}
              label={`+ ${r.name}`}
              onPress={() => {
                onChange([...value, { ingredient_id: r.id, kind }]);
                setQuery('');
              }}
            />
          ))}
        </Row>
      ) : needle ? (
        <Body tone="muted" size={15}>Pas d’ingrédient « {query} » dans la liste pour l’instant.</Body>
      ) : null}
    </Stack>
  );
}
