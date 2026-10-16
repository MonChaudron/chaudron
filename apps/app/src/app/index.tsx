import { Redirect } from 'expo-router';

import { useSession } from '../lib/session';
import { Loading, Screen } from '../ui';

/** Aiguillage : connexion, création du foyer, ou le foyer. */
export default function Home() {
  const { me } = useSession();
  if (me === undefined) {
    return (
      <Screen header={false}>
        <Loading />
      </Screen>
    );
  }
  if (!me) return <Redirect href="/connexion" />;
  if (me.households.length === 0) return <Redirect href="/foyer/nouveau" />;
  return <Redirect href="/foyer" />;
}
