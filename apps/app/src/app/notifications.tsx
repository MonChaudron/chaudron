import { space } from '@chaudron/domain';
import { Link, Redirect, Stack as Router } from 'expo-router';
import { useEffect } from 'react';
import { View } from 'react-native';

import { api } from '../lib/api';
import { useSession } from '../lib/session';
import { useTheme } from '../lib/theme';
import { useLoad } from '../lib/useLoad';
import { Body, Card, ErrorText, Loading, Screen, Stack, Title } from '../ui';

const when = (iso: string) => new Intl.DateTimeFormat('fr-FR', { day: 'numeric', month: 'long', hour: '2-digit', minute: '2-digit' }).format(new Date(iso));

/** Ce que sont devenues tes propositions et tes retours. Ouvrir la liste les marque lues. */
export default function Notifications() {
  const { me, refresh } = useSession();
  const { palette } = useTheme();
  const list = useLoad(() => api.GET('/api/v1/me/notifications'), []);
  const unread = list.data?.some((n) => !n.read);
  useEffect(() => {
    if (!unread) return;
    void api.POST('/api/v1/me/notifications/read', { body: {} }).then(() => refresh());
  }, [unread, refresh]);
  if (me === null) return <Redirect href="/connexion" />;
  return (
    <Screen>
      <Router.Screen options={{ title: 'Notifications · Chaudron' }} />
      <Stack gap={space[1]}>
        <Title>Notifications</Title>
        <Body tone="muted" size={16}>Ce que sont devenues tes propositions et tes retours à la communauté.</Body>
      </Stack>
      <ErrorText>{list.error}</ErrorText>
      {!list.data ? <Loading /> : null}
      {list.data?.length === 0 ? <Body tone="muted">Rien pour l’instant.</Body> : null}
      <View style={{ gap: space[3], maxWidth: 720 }}>
        {list.data?.map((n) => (
          <Card key={n.id} style={{ borderLeftWidth: n.read ? 0 : 4, borderLeftColor: palette.link }}>
            <Body size={16}>{n.text}</Body>
            <Body tone="muted" size={14}>{when(n.created_at)}</Body>
            {n.recipe_slug ? (
              <Link href={`/recettes/${n.recipe_slug}`} style={{ textDecorationLine: 'underline' }}>
                <Body size={15}>Voir la recette</Body>
              </Link>
            ) : null}
          </Card>
        ))}
      </View>
    </Screen>
  );
}
