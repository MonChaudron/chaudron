import { type ClientInfo, errorMessage } from '@chaudron/api';
import { scopes as scopeLabels, space } from '@chaudron/domain';
import { Stack as Router, useLocalSearchParams } from 'expo-router';
import { useMemo, useState } from 'react';

import { api } from '../../lib/api';
import { LoginOptions } from '../../lib/LoginOptions';
import { useSession } from '../../lib/session';
import { useLoad } from '../../lib/useLoad';
import { AuthLayout, Body, Button, Card, ErrorText, LinkButton, Loading, Pill, Row, Stack, Tag, Title } from '../../ui';

/** Ce qu'Amazon envoie (RFC 6749 §4.1.1). */
interface Request {
  client_id: string;
  redirect_uri: string;
  state?: string;
}

const KEY = 'chaudron.oauth';

function forget() {
  try {
    sessionStorage.removeItem(KEY);
  } catch {
    // rien à effacer
  }
}

/**
 * La demande en cours : celle de l'adresse, gardée le temps de la connexion (le `state` d'Amazon est trop long pour
 * passer par `return_to`), sinon celle gardée.
 */
function useRequest(): Request | null {
  const { client_id, redirect_uri, state } = useLocalSearchParams<{ client_id?: string; redirect_uri?: string; state?: string }>();
  return useMemo(() => {
    try {
      if (client_id && redirect_uri) {
        const fresh: Request = { client_id, redirect_uri, state };
        sessionStorage.setItem(KEY, JSON.stringify(fresh));
        return fresh;
      }
      const saved = sessionStorage.getItem(KEY);
      return saved ? (JSON.parse(saved) as Request) : null;
    } catch {
      // Sans stockage, la demande vaut pour cette page seulement.
      return client_id && redirect_uri ? { client_id, redirect_uri, state } : null;
    }
  }, [client_id, redirect_uri, state]);
}

/** Retour à l'application avec ces paramètres (`code`, ou `error`), et son `state`. */
function back(request: Request, query: Record<string, string>) {
  forget();
  const params = new URLSearchParams(query);
  if (request.state) params.set('state', request.state);
  const separator = request.redirect_uri.includes('?') ? '&' : '?';
  globalThis.location?.assign(`${request.redirect_uri}${separator}${params}`);
}

/** « Autoriser Alexa » : la page qu'ouvre l'appli Alexa pour lier le compte (liaison OAuth2). */
export default function Authorize() {
  const request = useRequest();
  const { me } = useSession();
  const client = useLoad<ClientInfo>(
    async () =>
      request
        ? api.GET('/api/v1/oauth/client', { params: { query: { client_id: request.client_id, redirect_uri: request.redirect_uri } } })
        : {},
    [request?.client_id, request?.redirect_uri],
  );
  const households = me?.households ?? [];
  const [chosen, setChosen] = useState<string | null>(null);
  const household = chosen ?? households[0]?.id ?? null;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const allow = async () => {
    if (!request || !household) return;
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/oauth/authorize', {
      body: { client_id: request.client_id, redirect_uri: request.redirect_uri, state: request.state, household_id: household },
    });
    if (!data) {
      setBusy(false);
      return setError(errorMessage(failure));
    }
    forget();
    globalThis.location?.assign(data.redirect_to);
  };

  const info = client.data;
  const name = info?.name ?? 'L’application';
  return (
    <AuthLayout>
      <Router.Screen options={{ title: `Autoriser ${name} · Chaudron` }} />
      <Card>
        {!request ? (
          <>
            <Title>Lien incomplet</Title>
            <Body tone="muted">Relance la liaison depuis l’appli Alexa.</Body>
          </>
        ) : client.error ? (
          <>
            <Title>Application inconnue</Title>
            <Body tone="muted">Cette demande ne vient pas d’une application que Chaudron connaît. Rien n’a été partagé.</Body>
          </>
        ) : !info || me === undefined ? (
          <Loading />
        ) : me === null ? (
          <>
            <Tag>{name}</Tag>
            <Title>Lier {name} à Chaudron</Title>
            <Body tone="muted">Connecte-toi : {name} pourra lire les menus et les recettes de ton foyer.</Body>
            <LoginOptions returnTo="/oauth/authorize" />
          </>
        ) : households.length === 0 ? (
          <>
            <Title>Pas encore de foyer</Title>
            <Body tone="muted">Crée ton foyer dans Chaudron, puis relance la liaison depuis l’appli Alexa.</Body>
            <LinkButton kind="primary" label="Créer mon foyer" href="/foyer/nouveau" />
          </>
        ) : (
          <>
            <Tag>{name}</Tag>
            <Title>{`Autoriser ${name} ?`}</Title>
            <Body tone="muted">
              {name} pourra lire les menus et les recettes du foyer, et te guider pendant la cuisson. Rien d’autre. Tu
              pourras retirer cet accès dans Le foyer › Accès.
            </Body>
            <Stack gap={space[1]}>
              {info.scopes.map((scope) => (
                <Body key={scope} size={16}>
                  · {scopeLabels[scope]}
                </Body>
              ))}
            </Stack>
            {households.length > 1 ? (
              <Stack gap={space[2]}>
                <Body size={16}>Pour quel foyer ?</Body>
                <Row>
                  {households.map((h) => (
                    <Pill key={h.id} role="radio" label={h.name} selected={h.id === household} onPress={() => setChosen(h.id)} />
                  ))}
                </Row>
              </Stack>
            ) : (
              <Body size={16}>Foyer « {households[0]?.name} »</Body>
            )}
            <ErrorText>{error}</ErrorText>
            <Button kind="primary" label={`Autoriser ${name}`} busy={busy} onPress={allow} />
            <Button kind="quiet" label="Refuser" onPress={() => back(request, { error: 'access_denied' })} />
          </>
        )}
      </Card>
    </AuthLayout>
  );
}
