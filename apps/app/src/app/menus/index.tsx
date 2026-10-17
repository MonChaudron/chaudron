import { nextMenuStart, shortDateText, space, todayIso } from '@chaudron/domain';
import { Redirect, Stack as Router, useRouter } from 'expo-router';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useLoad } from '../../lib/useLoad';
import { Body, Button, Card, ErrorText, ListRow, Loading, Row, Screen, Title } from '../../ui';

export default function Menus() {
  const { me } = useSession();
  const home = me?.households[0];
  if (me === null) return <Redirect href="/connexion" />;
  if (me && !home) return <Redirect href="/foyer/nouveau" />;
  if (!home) {
    return (
      <Screen>
        <Loading />
      </Screen>
    );
  }
  return <History householdId={home.id} />;
}

/** Les menus du foyer, du plus récent au plus ancien. */
function History({ householdId }: { householdId: string }) {
  const router = useRouter();
  const menus = useLoad(() => api.GET('/api/v1/households/{id}/menus', { params: { path: { id: householdId } } }), [householdId]);
  const today = todayIso();
  const list = menus.data;
  return (
    <Screen>
      <Router.Screen options={{ title: 'Menus · Chaudron' }} />
      <Row style={{ justifyContent: 'space-between' }}>
        <Title>Menus</Title>
        <Button kind="primary" label="Nouveau menu" onPress={() => router.push(`/menus/${nextMenuStart(list ?? [], today)}?nouveau=1`)} />
      </Row>
      {menus.error ? <ErrorText>{menus.error}</ErrorText> : null}
      {!list ? (
        menus.error ? null : <Loading />
      ) : list.length === 0 ? (
        <Card>
          <Body>Aucun menu pour l’instant. Compose le premier, ou demande à Claude de publier celui de la semaine.</Body>
        </Card>
      ) : (
        <Card style={{ gap: 0, paddingVertical: space[2] }}>
          {list.map((menu, i) => {
            const span = menu.first_date && menu.last_date ? `Du ${shortDateText(menu.first_date)} au ${shortDateText(menu.last_date)}` : 'Aucun plat';
            const current = !!menu.first_date && !!menu.last_date && menu.first_date <= today && today <= menu.last_date;
            return (
              <ListRow
                key={menu.week_start}
                title={menu.title || `Menu du ${shortDateText(menu.week_start)}`}
                detail={`${span} · ${menu.slot_count} plat${menu.slot_count > 1 ? 's' : ''}${current ? ' · en cours' : ''}`}
                onPress={() => router.push(`/menus/${menu.week_start}`)}
                last={i === list.length - 1}
              />
            );
          })}
        </Card>
      )}
    </Screen>
  );
}
