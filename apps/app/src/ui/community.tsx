// La recette et la communauté, sur la fiche : proposer une recette du foyer, copier une recette de la communauté,
// suivre l'originale d'une copie.
import type { Recipe } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { space } from '@chaudron/domain';
import { Link, useRouter } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';

import { api } from '../lib/api';
import { Body, Button, Card, ErrorText, Field, Row, Title } from './index';

export function CommunityCard({ householdId, recipe, onChange }: { householdId: string; recipe: Recipe; onChange: () => void }) {
  if (recipe.community) return <CopyCard householdId={householdId} recipe={recipe} />;
  if (recipe.copied_from) return <CopiedCard householdId={householdId} recipe={recipe} />;
  return <SubmitCard householdId={householdId} recipe={recipe} onChange={onChange} />;
}

/** Une recette de la communauté : la copier pour la faire à sa façon (elle reste liée à l'originale). */
function CopyCard({ householdId, recipe }: { householdId: string; recipe: Recipe }) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const copy = async () => {
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/recipes/{slug}/copy', {
      params: { path: { id: householdId, slug: recipe.slug } },
      body: {},
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    router.push(`/recettes/${data.slug}`);
  };
  return (
    <Card>
      <Title level={2}>Recette de la communauté</Title>
      <Body tone="muted" size={15}>
        Tu profites de chacune de ses améliorations. Pour la faire à ta façon, copie-la chez vous : elle garde le lien avec
        l’originale, et tu verras ce qui y change.
      </Body>
      <ErrorText>{error}</ErrorText>
      <View style={{ alignSelf: 'flex-start' }}>
        <Button label="Copier chez nous" busy={busy} onPress={copy} />
      </View>
    </Card>
  );
}

/** Une copie : d'après quelle recette, ce qui y a changé depuis ; proposer ses changements à l'originale. */
function CopiedCard({ householdId, recipe }: { householdId: string; recipe: Recipe }) {
  const from = recipe.copied_from;
  const [sent, setSent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const propose = async () => {
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/recipes/{slug}/propose-changes', {
      params: { path: { id: householdId, slug: recipe.slug } },
      body: {},
    });
    if (!data) return setError(errorMessage(failure));
    setSent(`${data.length} retour${data.length > 1 ? 's' : ''} envoyé${data.length > 1 ? 's' : ''} à l’admin.`);
  };
  if (!from) return null;
  return (
    <Card>
      <Title level={2}>Ta version</Title>
      <Body size={16}>
        D’après{' '}
        <Link href={`/recettes/${from.slug}`} style={{ textDecorationLine: 'underline' }}>
          {from.title}
        </Link>
        , version {from.version}.
      </Body>
      {from.behind ? (
        <Row gap={space[2]}>
          <Body tone="muted" size={15}>L’originale a changé depuis (version {from.current_version}).</Body>
          <Link href={`/recettes/originale?slug=${recipe.slug}`} style={{ textDecorationLine: 'underline' }}>
            <Body size={15}>Voir et reprendre</Body>
          </Link>
        </Row>
      ) : (
        <Body tone="muted" size={15}>Elle suit la version courante de l’originale.</Body>
      )}
      <ErrorText>{error}</ErrorText>
      {sent ? (
        <Body size={15}>{sent}</Body>
      ) : (
        <View style={{ alignSelf: 'flex-start' }}>
          <Button kind="quiet" label="Proposer mes changements à la communauté" onPress={propose} />
        </View>
      )}
    </Card>
  );
}

/** Une recette du foyer : la proposer à la communauté, avec un mot ; l'admin de l'instance choisit. */
function SubmitCard({ householdId, recipe, onChange }: { householdId: string; recipe: Recipe; onChange: () => void }) {
  const [open, setOpen] = useState(false);
  const [message, setMessage] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const status = recipe.submission?.status;
  const submit = async () => {
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/recipes/{slug}/submit', {
      params: { path: { id: householdId, slug: recipe.slug } },
      body: { message: message.trim() || null },
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    setOpen(false);
    onChange();
  };
  if (status === 'pending') {
    return (
      <Card>
        <Title level={2}>Proposée à la communauté</Title>
        <Body tone="muted" size={15}>En attente de l’admin de l’instance. Tu seras prévenu de sa réponse.</Body>
      </Card>
    );
  }
  return (
    <Card>
      <Title level={2}>La communauté</Title>
      {status === 'refused' ? (
        <Body size={15}>{`Pas retenue la dernière fois${recipe.submission?.review_message ? ` : « ${recipe.submission.review_message} »` : '.'}`}</Body>
      ) : (
        <Body tone="muted" size={15}>Une bonne recette ? Propose-la aux autres foyers de l’instance : l’admin choisit.</Body>
      )}
      {open ? (
        <View style={{ gap: space[2] }}>
          <Field label="Un mot pour l’admin" hint="Facultatif : ce qui la rend bonne, combien de fois vous l’avez faite." value={message} onChangeText={setMessage} multiline maxLength={2000} style={{ minHeight: 72, paddingTop: 12, textAlignVertical: 'top' }} />
          <ErrorText>{error}</ErrorText>
          <Row>
            <Button kind="primary" label="Proposer" busy={busy} onPress={submit} />
            <Button kind="quiet" label="Annuler" onPress={() => setOpen(false)} />
          </Row>
        </View>
      ) : (
        <View style={{ alignSelf: 'flex-start' }}>
          <Button label={status === 'refused' ? 'La proposer de nouveau' : 'Proposer à la communauté'} onPress={() => setOpen(true)} />
        </View>
      )}
    </Card>
  );
}
