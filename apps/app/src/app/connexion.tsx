import { Redirect, Stack as Router, useLocalSearchParams } from 'expo-router';

import { LoginOptions, loginErrors } from '../lib/LoginOptions';
import { useSession } from '../lib/session';
import { AuthLayout, Back, Body, Card, ErrorText, Loading, Title } from '../ui';

export default function Connexion() {
  const { me, offline } = useSession();
  const { erreur } = useLocalSearchParams<{ erreur?: string }>();
  if (me) return <Redirect href="/" />;

  return (
    <AuthLayout>
      <Router.Screen options={{ title: 'Connexion · Chaudron' }} />
      <Back href="/" label="Découvrir Chaudron" />
      <Card>
        {offline ? <ErrorText>Chaudron ne répond pas. Vérifie ta connexion.</ErrorText> : null}
        <Title>Les menus de la maison</Title>
        <Body tone="muted">Recettes adaptées à ton matériel, menus de la semaine et liste de courses partagée.</Body>
        {me === undefined ? (
          <Loading />
        ) : (
          <LoginOptions initialError={erreur ? (loginErrors[erreur] ?? 'Connexion impossible.') : null} />
        )}
      </Card>
    </AuthLayout>
  );
}
