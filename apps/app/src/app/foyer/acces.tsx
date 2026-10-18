import { type ApiTokenInfo, errorMessage, type LinkInfo, type Scope } from '@chaudron/api';
import { scopes as scopeLabels, space } from '@chaudron/domain';
import { Redirect, Stack as Router } from 'expo-router';
import { useState } from 'react';
import { Platform, View } from 'react-native';

import { api, apiBase } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useLoad } from '../../lib/useLoad';
import { Back, Body, Button, Card, Columns, ErrorText, Field, ListRow, Loading, Pill, Row, Screen, Stack, Title } from '../../ui';

/** Les droits proposés : les menus arrivent au M3. */
const offered: Scope[] = ['read', 'recipes:write'];

/**
 * Ce qui entre dans le foyer sans se connecter : les jetons d'API (Claude, un script) et les applications liées
 * (Alexa). Propriétaires seulement.
 */
export default function Access() {
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
  if (home.role !== 'owner') return <Redirect href="/foyer" />;
  return <Tokens householdId={home.id} />;
}

function Tokens({ householdId }: { householdId: string }) {
  const path = { params: { path: { id: householdId } } };
  const tokens = useLoad(() => api.GET('/api/v1/households/{id}/tokens', path), [householdId]);
  const links = useLoad(() => api.GET('/api/v1/households/{id}/links', path), [householdId]);
  const [created, setCreated] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const address = apiBase || (Platform.OS === 'web' ? window.location.origin : '');

  const revoke = async (token: ApiTokenInfo) => {
    const { response, error: failure } = await api.DELETE('/api/v1/households/{id}/tokens/{token_id}', {
      params: { path: { id: householdId, token_id: token.id } },
    });
    setError(response.ok ? null : errorMessage(failure));
    await tokens.reload();
  };

  const unlink = async (link: LinkInfo) => {
    const { response, error: failure } = await api.DELETE('/api/v1/households/{id}/links/{link_id}', {
      params: { path: { id: householdId, link_id: link.id } },
    });
    setError(response.ok ? null : errorMessage(failure));
    await links.reload();
  };

  return (
    <Screen>
      <Router.Screen options={{ title: 'Accès · Chaudron' }} />
      <Back href="/foyer" label="Le foyer" />
      <Title>Accès</Title>
      <Body tone="muted" style={{ maxWidth: 720 }}>
        Un jeton d’accès laisse Claude, ou un script, publier les recettes du foyer sans se connecter ; Alexa se lie
        depuis son appli, avec ton compte Chaudron. Chaque accès ne vaut que pour ce foyer, et tu peux le retirer à tout
        moment.
      </Body>

      <Columns weights={[1, 1]}>
        <Stack gap={space[5]}>
          {created ? (
            <Card>
              <Title level={2}>Ton nouveau jeton</Title>
              <Body size={16}>Copie-le maintenant : il ne sera plus affiché. Donne à Claude ces trois valeurs.</Body>
              <Copyable label="Adresse (CHAUDRON_URL)" value={address} />
              <Copyable label="Foyer (CHAUDRON_HOUSEHOLD)" value={householdId} />
              <Copyable label="Jeton (CHAUDRON_TOKEN)" value={created} />
              <Button kind="quiet" label="C’est noté" onPress={() => setCreated(null)} />
            </Card>
          ) : (
            <NewToken
              householdId={householdId}
              onCreated={async (token) => {
                setCreated(token);
                await tokens.reload();
              }}
            />
          )}
        </Stack>

        <Stack gap={space[5]}>
          <Card>
            <Title level={2}>Jetons du foyer</Title>
            <ErrorText>{error ?? tokens.error}</ErrorText>
            {tokens.data === undefined ? <Loading /> : null}
            {tokens.data?.length === 0 ? <Body tone="muted" size={16}>Aucun jeton pour l’instant.</Body> : null}
            <View>
              {tokens.data?.map((token, i) => (
                <View key={token.id} style={{ gap: space[1] }}>
                  <ListRow
                    title={token.name}
                    detail={[
                      token.scopes.map((s) => scopeLabels[s]).join(', '),
                      token.last_used_at ? `servi le ${formatDate(token.last_used_at)}` : 'jamais servi',
                    ].join(' — ')}
                    last
                  />
                  <Row style={{ paddingBottom: i === (tokens.data?.length ?? 0) - 1 ? 0 : space[3] }}>
                    <Button kind="quiet" label="Retirer ce jeton" onPress={() => revoke(token)} />
                  </Row>
                </View>
              ))}
            </View>
          </Card>
          <Card>
            <Title level={2}>Applications liées</Title>
            <ErrorText>{links.error}</ErrorText>
            {links.data === undefined && !links.error ? <Loading /> : null}
            {links.data?.length === 0 ? (
              <Body tone="muted" size={16}>
                Aucune. Pour Alexa : dans l’appli Alexa, active la skill « Chaudron » et lie ton compte.
              </Body>
            ) : null}
            <View>
              {links.data?.map((link, i) => (
                <View key={link.id} style={{ gap: space[1] }}>
                  <ListRow
                    title={link.client}
                    detail={[
                      `liée par ${link.user}`,
                      link.last_used_at ? `servie le ${formatDate(link.last_used_at)}` : 'jamais servie',
                    ].join(' — ')}
                    last
                  />
                  <Row style={{ paddingBottom: i === (links.data?.length ?? 0) - 1 ? 0 : space[3] }}>
                    <Button kind="quiet" label="Retirer cette liaison" onPress={() => unlink(link)} />
                  </Row>
                </View>
              ))}
            </View>
          </Card>
        </Stack>
      </Columns>
    </Screen>
  );
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString('fr-FR', { day: 'numeric', month: 'long', hour: '2-digit', minute: '2-digit' });
}

function NewToken({ householdId, onCreated }: { householdId: string; onCreated: (token: string) => Promise<void> }) {
  const [name, setName] = useState('Claude');
  const [chosen, setChosen] = useState<Scope[]>(offered);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const create = async () => {
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/tokens', {
      params: { path: { id: householdId } },
      body: { name, scopes: chosen },
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    setError(null);
    await onCreated(data.token);
  };

  return (
    <Card>
      <Title level={2}>Créer un jeton</Title>
      <Field label="Nom" hint="Pour le reconnaître dans la liste : « Claude », « Script du dimanche »." value={name} onChangeText={setName} maxLength={80} />
      <Stack gap={space[2]}>
        <Body size={16}>Ce qu’il peut faire</Body>
        <Row>
          {offered.map((scope) => (
            <Pill
              key={scope}
              label={scopeLabels[scope]}
              selected={chosen.includes(scope)}
              onPress={() => setChosen(chosen.includes(scope) ? chosen.filter((s) => s !== scope) : [...chosen, scope])}
            />
          ))}
        </Row>
      </Stack>
      <ErrorText>{error}</ErrorText>
      <Button kind="primary" label="Créer le jeton" busy={busy} disabled={!name.trim() || chosen.length === 0} onPress={create} />
    </Card>
  );
}

function Copyable({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Stack gap={space[2]}>
      <Field label={label} value={value} editable={false} selectTextOnFocus />
      <Row>
        <Button
          label={copied ? 'Copié' : 'Copier'}
          onPress={async () => {
            try {
              await navigator.clipboard.writeText(value);
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
