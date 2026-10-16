import { errorMessage } from '@chaudron/api';
import { Stack as Router, useLocalSearchParams, useRouter } from 'expo-router';
import { useState } from 'react';

import { api } from '../../lib/api';
import { LoginOptions } from '../../lib/LoginOptions';
import { useSession } from '../../lib/session';
import { useLoad } from '../../lib/useLoad';
import { AuthLayout, Body, Button, Card, ErrorText, LinkButton, Loading, Tag, Title } from '../../ui';

/** Page d'un lien d'invitation : qui invite, où, puis connexion (ou « rejoindre » si déjà connecté). */
export default function Invitation() {
  const { token } = useLocalSearchParams<{ token: string }>();
  const { me, refresh } = useSession();
  const router = useRouter();
  const [joining, setJoining] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const invite = useLoad(() => api.GET('/api/v1/invites/{token}', { params: { path: { token } } }), [token, me?.id]);

  const join = async () => {
    setJoining(true);
    const { data, error: failure } = await api.POST('/api/v1/invites/{token}/accept', { params: { path: { token } } });
    setJoining(false);
    if (!data) return setError(errorMessage(failure));
    await refresh();
    router.replace('/foyer');
  };

  const info = invite.data;
  return (
    <AuthLayout>
      <Router.Screen options={{ title: 'Invitation · Chaudron' }} />
      <Card>
        {!info && !invite.error ? <Loading /> : null}
        {invite.error ? (
          <>
            <Title>Lien inconnu</Title>
            <Body tone="muted">Ce lien d’invitation n’existe pas, ou il a été annulé.</Body>
          </>
        ) : null}
        {info?.joined_household_id ? (
          <>
            <Tag>Bienvenue</Tag>
            <Title>Tu fais partie du foyer « {info.household_name} »</Title>
            <LinkButton kind="primary" label="Voir le foyer" href="/foyer" />
          </>
        ) : null}
        {info && !info.joined_household_id && info.status !== 'pending' ? (
          <>
            <Title>Lien expiré</Title>
            <Body tone="muted">
              Ce lien n’est plus valable. Demande-en un nouveau à {info.invited_by ?? 'la personne qui t’a invitée'}.
            </Body>
          </>
        ) : null}
        {info && !info.joined_household_id && info.status === 'pending' ? (
          <>
            <Tag>Invitation</Tag>
            <Title>Rejoins le foyer « {info.household_name} »</Title>
            <Body tone="muted">
              {info.invited_by ?? 'Quelqu’un'} t’invite à partager les menus de la semaine, les recettes et la liste de courses.
            </Body>
            <ErrorText>{error}</ErrorText>
            {me ? <Button kind="primary" label="Rejoindre le foyer" busy={joining} onPress={join} /> : <LoginOptions invite={token} />}
            <Body tone="muted" size={15} style={{ textAlign: 'center' }}>
              Lien valable 7 jours, une seule fois.
            </Body>
          </>
        ) : null}
      </Card>
    </AuthLayout>
  );
}
