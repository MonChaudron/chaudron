// Page d'accueil hors connexion. Sur le web, `Landing.web.tsx` (la présentation de Chaudron) ; ailleurs, la
// connexion directement.
import { Redirect } from 'expo-router';

export function Landing() {
  return <Redirect href="/connexion" />;
}
