import { errorMessage, type HouseholdUser, type MemberKind } from '@chaudron/api';
import {
  equipment as equipmentLabels,
  formatTotal,
  label,
  memberKinds,
  portionsSummary,
  presenceSummary,
  roles,
  space,
} from '@chaudron/domain';
import { Redirect, Stack as Router, useRouter } from 'expo-router';
import { useMemo, useState } from 'react';
import { Platform, Pressable, Share, Text, View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useTheme } from '../../lib/theme';
import { useLoad } from '../../lib/useLoad';
import {
  Body,
  Button,
  Card,
  Choice,
  Columns,
  ErrorText,
  Field,
  LinkButton,
  ListRow,
  Loading,
  Row,
  Screen,
  Stack,
  Tag,
  Title,
  useWide,
} from '../../ui';

const kindOptions = (Object.keys(memberKinds) as MemberKind[]).map((value) => ({ value, label: memberKinds[value] }));

export default function Foyer() {
  const { me, refresh } = useSession();
  const home = me?.households[0];
  if (me === null) return <Redirect href="/connexion" />;
  if (me && !home) return <Redirect href="/foyer/nouveau" />;
  if (!me || !home) {
    return (
      <Screen>
        <Loading />
      </Screen>
    );
  }
  return <Household id={home.id} me={me.id} onLeft={refresh} />;
}

function Household({ id, me, onLeft }: { id: string; me: string; onLeft: () => Promise<unknown> }) {
  const path = { params: { path: { id } } };
  const household = useLoad(() => api.GET('/api/v1/households/{id}', path), [id]);
  const members = useLoad(() => api.GET('/api/v1/households/{id}/members', path), [id]);
  const owned = useLoad(() => api.GET('/api/v1/households/{id}/equipment', path), [id]);
  const ingredients = useLoad(() => api.GET('/api/v1/ingredients'), []);
  const ingredientNames = useMemo(
    () => new Map((ingredients.data ?? []).map((i) => [i.id, i.name])),
    [ingredients.data],
  );

  const wide = useWide();
  const data = household.data;
  if (!data) {
    return (
      <Screen>
        {household.error ? <ErrorText>{household.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }
  const owner = data.role === 'owner';

  return (
    <Screen>
      <Router.Screen options={{ title: `${data.name} · Chaudron` }} />
      <Stack gap={space[2]}>
        <Tag>{roles[data.role]}</Tag>
        <Title>{data.name}</Title>
      </Stack>

      <Columns weights={[3, 2]}>
        <Card>
          <Title level={2}>Qui mange</Title>
          {members.data === undefined ? <Loading /> : null}
          {members.data?.length === 0 ? <Body tone="muted">Personne pour l’instant : ajoute ceux qui mangent à la maison, enfants compris.</Body> : null}
          <View style={wide ? { flexDirection: 'row', flexWrap: 'wrap', gap: space[3] } : undefined}>
            {members.data?.map((member, index) => {
              const portions = portionsSummary(member.portions);
              const allergies = member.exclusions
                .filter((e) => e.kind === 'allergy')
                .map((e) => ingredientNames.get(e.ingredient_id) ?? '…');
              const detail = [memberKinds[member.kind], portions, presenceSummary(member.presence)].filter(Boolean).join(' · ');
              const alert = allergies.length ? `Allergie : ${allergies.join(', ').toLowerCase()}` : undefined;
              if (wide) {
                return <MemberTile key={member.id} href={`/foyer/membres/${member.id}`} name={member.name} detail={detail} alert={alert} />;
              }
              return (
                <ListRowLink
                  key={member.id}
                  href={`/foyer/membres/${member.id}`}
                  title={member.name}
                  detail={detail}
                  alert={alert}
                  last={index === (members.data?.length ?? 0) - 1}
                />
              );
            })}
          </View>
          <AddMember householdId={id} />
        </Card>

        <>
          <Card>
            <Title level={2}>Le matériel</Title>
            {owned.data?.codes.length ? (
              <Row>
                {owned.data.codes.map((code) => (
                  <Tag key={code}>{label(equipmentLabels, `equipment.${code}`)}</Tag>
                ))}
              </Row>
            ) : (
              <Body tone="muted">Indique ce que tu as en cuisine : les recettes s’adapteront (Cookeo, four, Thermomix…).</Body>
            )}
            <LinkButton label="Choisir le matériel" href="/foyer/equipements" />
          </Card>

          <Budget householdId={id} cents={data.weekly_budget_cents ?? null} owner={owner} onChanged={household.reload} />

          <Accounts
            householdId={id}
            users={data.users}
            me={me}
            owner={owner}
            onChanged={household.reload}
            onLeft={onLeft}
          />

          {owner ? (
            <Card>
              <Title level={2}>Accès</Title>
              <Body tone="muted" size={16}>Claude publie les recettes et les menus ; Alexa lit le menu et guide la cuisson.</Body>
              <LinkButton label="Gérer les accès" href="/foyer/acces" />
            </Card>
          ) : null}
        </>
      </Columns>
    </Screen>
  );
}

/** Bureau : fiche de membre dans la grille, lien vers sa fiche complète. */
/** Le plafond d'une semaine de menu : Claude le respecte, la page du menu prévient s'il est dépassé. */
function Budget({ householdId, cents, owner, onChanged }: { householdId: string; cents: number | null; owner: boolean; onChanged: () => void }) {
  const [text, setText] = useState(cents == null ? '' : String(Math.round(cents / 100)));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const value = text.trim();
  const valid = value === '' || /^\d{1,5}$/.test(value);
  const save = async () => {
    setBusy(true);
    setError(null);
    const { error: failure } = await api.PATCH('/api/v1/households/{id}', {
      params: { path: { id: householdId } },
      body: { weekly_budget_cents: value === '' ? null : Number(value) * 100 },
    });
    setBusy(false);
    if (failure) setError(errorMessage(failure));
    else onChanged();
  };
  return (
    <Card>
      <Title level={2}>Budget</Title>
      <Body tone="muted" size={16}>
        {cents == null ? 'Pas de plafond : Claude compose des menus raisonnables.' : `Pas de menu à plus de ${formatTotal(cents)} la semaine.`}
      </Body>
      {owner ? (
        <>
          <Field label="Plafond d’une semaine, en euros" hint="Vide : pas de plafond." value={text} onChangeText={setText} keyboardType="number-pad" placeholder="140" />
          <ErrorText>{error}</ErrorText>
          <Button label="Enregistrer" busy={busy} disabled={!valid} onPress={() => void save()} />
        </>
      ) : null}
    </Card>
  );
}

function MemberTile({ href, name, detail, alert }: { href: string; name: string; detail: string; alert?: string }) {
  const router = useRouter();
  const { palette, radius, fonts, weights } = useTheme();
  return (
    <Pressable
      onPress={() => router.push(href as never)}
      accessibilityRole="button"
      accessibilityLabel={name}
      style={({ pressed, hovered }: { pressed: boolean; hovered?: boolean }) => ({
        flexGrow: 1,
        flexBasis: 240,
        padding: space[4],
        gap: space[1],
        borderRadius: radius.card - 6,
        borderWidth: 1.5,
        borderColor: hovered ? palette.muted : palette.line,
        backgroundColor: palette.bg,
        opacity: pressed ? 0.8 : 1,
      })}
    >
      <Text style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: 19, color: palette.text }}>{name}</Text>
      <Body tone="muted" size={15}>{detail}</Body>
      {alert ? <Body tone="alert" size={15}>{alert}</Body> : null}
    </Pressable>
  );
}

/** Ligne de membre : lien vers sa fiche. */
function ListRowLink({ href, title, detail, alert, last }: { href: string; title: string; detail: string; alert?: string; last: boolean }) {
  const router = useRouter();
  return (
    <ListRow
      title={title}
      detail={detail}
      extra={alert ? <Body tone="alert" size={15}>{alert}</Body> : undefined}
      onPress={() => router.push(href as never)}
      last={last}
    />
  );
}

function AddMember({ householdId }: { householdId: string }) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [kind, setKind] = useState<MemberKind>('adult');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return <Button label="Ajouter quelqu’un" onPress={() => setOpen(true)} />;

  const add = async () => {
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/members', {
      params: { path: { id: householdId } },
      body: { name, kind },
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    router.push(`/foyer/membres/${data.id}`);
  };

  return (
    <Stack>
      <Field label="Prénom" value={name} onChangeText={setName} maxLength={80} autoFocus />
      <Choice label="Type" options={kindOptions} value={kind} onChange={setKind} />
      <Body tone="muted" size={15}>Le barème de portions suit le type ; tu pourras l’ajuster ensuite.</Body>
      <ErrorText>{error}</ErrorText>
      <Row>
        <View style={{ flex: 1 }}>
          <Button kind="primary" label="Ajouter" busy={busy} disabled={!name.trim()} onPress={add} />
        </View>
        <Button kind="quiet" label="Annuler" onPress={() => setOpen(false)} />
      </Row>
    </Stack>
  );
}

function Accounts({
  householdId,
  users,
  me,
  owner,
  onChanged,
  onLeft,
}: {
  householdId: string;
  users: HouseholdUser[];
  me: string;
  owner: boolean;
  onChanged: () => Promise<unknown>;
  onLeft: () => Promise<unknown>;
}) {
  const router = useRouter();
  const [error, setError] = useState<string | null>(null);
  const invites = useLoad(
    () =>
      owner
        ? api.GET('/api/v1/households/{id}/invites', { params: { path: { id: householdId } } })
        : Promise.resolve({ data: [] }),
    [householdId, owner],
  );

  const setRole = async (user: HouseholdUser, role: 'owner' | 'member') => {
    const { error: failure, response } = await api.PATCH('/api/v1/households/{id}/users/{user_id}', {
      params: { path: { id: householdId, user_id: user.user_id } },
      body: { role },
    });
    setError(response.ok ? null : errorMessage(failure));
    await onChanged();
  };
  const remove = async (user: HouseholdUser) => {
    const { error: failure, response } = await api.DELETE('/api/v1/households/{id}/users/{user_id}', {
      params: { path: { id: householdId, user_id: user.user_id } },
    });
    if (!response.ok) return setError(errorMessage(failure));
    setError(null);
    if (user.user_id === me) {
      await onLeft();
      router.replace('/');
    } else await onChanged();
  };

  return (
    <Card>
      <Title level={2}>Les comptes</Title>
      <Body tone="muted" size={16}>Ceux qui se connectent au foyer. Tout le monde modifie la cuisine ; les propriétaires gèrent aussi les comptes.</Body>
      <ErrorText>{error}</ErrorText>
      <View>
        {users.map((user, index) => {
          const self = user.user_id === me;
          return (
            <View key={user.user_id} style={{ gap: space[2], paddingBottom: space[2] }}>
              <ListRow
                title={self ? `${user.display_name} (toi)` : user.display_name}
                detail={[roles[user.role], user.email].filter(Boolean).join(' · ')}
                last={index === users.length - 1 && !(owner && !self) && !(self && !owner)}
              />
              {owner && !self ? (
                <Row>
                  {user.role === 'member' ? (
                    <Button kind="quiet" label="Rendre propriétaire" onPress={() => setRole(user, 'owner')} />
                  ) : (
                    <Button kind="quiet" label="Rendre membre" onPress={() => setRole(user, 'member')} />
                  )}
                  <Button kind="quiet" label="Retirer du foyer" onPress={() => remove(user)} />
                </Row>
              ) : null}
              {self && (!owner || users.filter((u) => u.role === 'owner').length > 1) ? (
                <Row>
                  {owner ? <Button kind="quiet" label="Ne plus être propriétaire" onPress={() => setRole(user, 'member')} /> : null}
                  <Button kind="quiet" label="Quitter le foyer" onPress={() => remove(user)} />
                </Row>
              ) : null}
            </View>
          );
        })}
      </View>
      {owner ? <Invite householdId={householdId} onCreated={invites.reload} /> : null}
      {owner && invites.data?.length ? (
        <Stack gap={space[2]}>
          <Body tone="muted" size={15}>Invitations en attente</Body>
          {invites.data.map((invite) => (
            <Row key={invite.id}>
              <Body size={16} style={{ flex: 1 }}>
                Lien du {new Date(invite.created_at).toLocaleDateString('fr-FR', { day: 'numeric', month: 'long' })}, valable jusqu’au{' '}
                {new Date(invite.expires_at).toLocaleDateString('fr-FR', { day: 'numeric', month: 'long' })}
              </Body>
              <Button
                kind="quiet"
                label="Annuler"
                onPress={async () => {
                  await api.DELETE('/api/v1/households/{id}/invites/{invite_id}', {
                    params: { path: { id: householdId, invite_id: invite.id } },
                  });
                  await invites.reload();
                }}
              />
            </Row>
          ))}
        </Stack>
      ) : null}
    </Card>
  );
}

function Invite({ householdId, onCreated }: { householdId: string; onCreated: () => Promise<unknown> }) {
  const [url, setUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const create = async () => {
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/invites', {
      params: { path: { id: householdId } },
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    setUrl(data.url);
    await onCreated();
  };

  if (!url) {
    return (
      <Stack>
        <ErrorText>{error}</ErrorText>
        <Button kind="primary" label="Inviter quelqu’un" busy={busy} onPress={create} />
      </Stack>
    );
  }

  const canShare = Platform.OS !== 'web' || typeof navigator.share === 'function';
  return (
    <Stack>
      <Body size={16}>Envoie ce lien à la personne invitée. Il sert une seule fois, pendant 7 jours.</Body>
      <Field label="Lien d’invitation" value={url} editable={false} selectTextOnFocus />
      <Row>
        {canShare ? (
          <View style={{ flex: 1 }}>
            <Button
              kind="primary"
              label="Partager…"
              onPress={() => {
                const message = `Rejoins notre foyer sur Chaudron : ${url}`;
                if (Platform.OS === 'web') void navigator.share({ title: 'Chaudron', text: message, url }).catch(() => undefined);
                else void Share.share({ message });
              }}
            />
          </View>
        ) : null}
        <Button
          label={copied ? 'Copié' : 'Copier'}
          onPress={async () => {
            try {
              await navigator.clipboard.writeText(url);
              setCopied(true);
            } catch {
              setCopied(false);
            }
          }}
        />
      </Row>
    </Stack>
  );
}
