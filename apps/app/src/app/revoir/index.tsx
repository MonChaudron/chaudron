import type { Submission } from '@chaudron/api';
import { space } from '@chaudron/domain';
import { Redirect, Stack as Router } from 'expo-router';
import { View } from 'react-native';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useLoad } from '../../lib/useLoad';
import { Body, Card, ErrorText, LinkButton, Loading, Row, Screen, Stack, Title } from '../../ui';
import { ReviewFeedback } from '../../ui/review';

const when = (iso: string) => new Intl.DateTimeFormat('fr-FR', { day: 'numeric', month: 'long' }).format(new Date(iso));

/** « À revoir » : ce qui attend l'admin de l'instance. */
export default function Review() {
  const { me } = useSession();
  const review = useLoad(() => api.GET('/api/v1/community/review'), []);
  if (me === null) return <Redirect href="/connexion" />;
  if (me && !me.instance_admin) return <Redirect href="/" />;
  const data = review.data;
  return (
    <Screen>
      <Router.Screen options={{ title: 'À revoir · Chaudron' }} />
      <Stack gap={space[1]}>
        <Title>À revoir</Title>
        <Body tone="muted" size={16}>Les recettes proposées et les retours de la communauté. L’auteur est prévenu de ta réponse.</Body>
      </Stack>
      <ErrorText>{review.error}</ErrorText>
      {!data ? <Loading /> : null}
      {data ? (
        <Stack gap={space[3]}>
          <Title level={2}>{`Recettes proposées (${data.submissions.length})`}</Title>
          {data.submissions.length === 0 ? <Body tone="muted">Aucune pour l’instant.</Body> : null}
          <View style={{ gap: space[3], maxWidth: 820 }}>
            {data.submissions.map((s) => (
              <SubmissionCard key={s.id} submission={s} />
            ))}
          </View>
          <Title level={2}>{`Retours (${data.recipes.reduce((n, r) => n + r.feedback.length, 0)})`}</Title>
          {data.recipes.length === 0 ? <Body tone="muted">Aucun retour à revoir.</Body> : null}
          <View style={{ gap: space[3], maxWidth: 820 }}>
            {data.recipes.map((group) => (
              <ReviewFeedback key={group.slug} group={group} householdId={me?.households[0]?.id} onChange={() => void review.reload()} />
            ))}
          </View>
        </Stack>
      ) : null}
    </Screen>
  );
}

function SubmissionCard({ submission: s }: { submission: Submission }) {
  return (
    <Card>
      <Title level={2}>{s.title}</Title>
      <Body tone="muted" size={15}>{`${s.household_name ?? 'Un foyer'}, par ${s.author_name}, le ${when(s.created_at)}`}</Body>
      {s.message ? <Body size={16}>« {s.message} »</Body> : null}
      <Row>
        <LinkButton kind="primary" label="Voir et répondre" href={`/revoir/proposition?id=${s.id}`} />
      </Row>
    </Card>
  );
}
