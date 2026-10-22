// « À revoir » : les retours de la communauté, par recette. Proposition : appliquer, modifier puis appliquer,
// refuser ; commentaire : publier, fermer, refuser. Une proposition dont l'étape a changé ne s'applique pas à l'aveugle.
import type { Feedback, ReviewRecipe } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { recipeToInput, space } from '@chaudron/domain';
import { Link } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';

import { api } from '../lib/api';
import { useTheme } from '../lib/theme';
import { ProposedChanges } from './discussion';
import { Body, Button, Card, ErrorText, Field, Row, Tag, Title } from './index';
import { TriedToSubstitution } from './substitutions';

const kindLabels = { comment: 'Commentaire', proposal: 'Proposition', tried: 'Variante tentée' } as const;

export function ReviewFeedback({ group, householdId, onChange }: { group: ReviewRecipe; householdId?: string; onChange: () => void }) {
  return (
    <Card>
      <Row style={{ justifyContent: 'space-between' }}>
        <Title level={2}>{group.title}</Title>
        <Link href={`/recettes/${group.slug}`} style={{ textDecorationLine: 'underline' }}>
          <Body size={15}>La recette</Body>
        </Link>
      </Row>
      <View style={{ gap: space[3] }}>
        {group.feedback.map((f) => (
          <Item key={f.id} feedback={f} slug={group.slug} householdId={householdId} onChange={onChange} />
        ))}
      </View>
    </Card>
  );
}

type Action = 'publish' | 'apply' | 'close' | 'refuse';

function Item({ feedback: f, slug, householdId, onChange }: { feedback: Feedback; slug: string; householdId?: string; onChange: () => void }) {
  const { palette, radius } = useTheme();
  const [word, setWord] = useState('');
  const [refusing, setRefusing] = useState(false);
  const [editing, setEditing] = useState(false);
  const proposed = (f.changes ?? {}) as Record<string, { from?: unknown; to?: unknown }>;
  const [text, setText] = useState(String(proposed.text?.to ?? proposed.text?.from ?? ''));
  const [minutes, setMinutes] = useState(typeof proposed.timer_s?.to === 'number' ? String(Math.round(proposed.timer_s.to / 60)) : '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const act = async (action: Action, recipe?: unknown) => {
    setBusy(true);
    const path = { params: { path: { feedback_id: f.id } }, body: { message: word.trim() || null, recipe: (recipe ?? null) as never } };
    const { response, error: failure } =
      action === 'publish'
        ? await api.POST('/api/v1/community/feedback/{feedback_id}/publish', path)
        : action === 'apply'
          ? await api.POST('/api/v1/community/feedback/{feedback_id}/apply', path)
          : action === 'close'
            ? await api.POST('/api/v1/community/feedback/{feedback_id}/close', path)
            : await api.POST('/api/v1/community/feedback/{feedback_id}/refuse', path);
    setBusy(false);
    if (!response.ok) return setError(errorMessage(failure));
    onChange();
  };

  /** « Modifier puis appliquer » : la recette telle qu'elle est, l'étape revue par l'admin. */
  const applyEdited = async () => {
    if (!householdId || !f.element_key) return;
    const { data, error: failure } = await api.GET('/api/v1/households/{id}/recipes/{slug}', { params: { path: { id: householdId, slug } } });
    if (!data) return setError(errorMessage(failure));
    const input = recipeToInput(data);
    for (const v of input.variants) {
      for (const p of v.phases) {
        const target = p.key === f.element_key ? p : p.steps.find((s) => s.key === f.element_key);
        if (!target) continue;
        if (text.trim()) target.text = text.trim();
        const timer = Math.round(Number(minutes.replace(',', '.')) * 60);
        if (timer > 0) target.timer_s = timer;
      }
    }
    await act('apply', { ...input, slug: undefined });
  };

  return (
    <View style={{ gap: space[2], padding: space[3], borderRadius: radius.button, borderWidth: 1, borderColor: palette.line }}>
      <Row gap={space[2]}>
        <Tag tone={f.kind === 'proposal' ? 'strong' : 'neutral'}>{kindLabels[f.kind]}</Tag>
        {f.status === 'published' ? <Tag>Publiée</Tag> : null}
        {f.parent_id ? <Tag>Réponse</Tag> : null}
        {f.stale ? <Tag tone="warn" icon="alert">L’étape a changé depuis</Tag> : null}
      </Row>
      {f.kind === 'proposal' ? <ProposedChanges changes={f.changes} /> : null}
      <Body size={15}>{f.body}</Body>
      {f.kind === 'tried' ? <TriedToSubstitution feedback={f} onDone={onChange} /> : null}
      <Body tone="muted" size={13}>{`${f.author_name}${f.household_name ? ` (${f.household_name})` : ''}${f.supports ? ` · +${f.supports}` : ''}`}</Body>
      {editing ? (
        <View style={{ gap: space[2] }}>
          <Field label="L’étape, revue" value={text} onChangeText={setText} multiline style={{ minHeight: 96, paddingTop: 12, textAlignVertical: 'top' }} />
          <Field label="Minuteur (minutes, facultatif)" value={minutes} onChangeText={setMinutes} keyboardType="decimal-pad" />
        </View>
      ) : null}
      <Field label={refusing ? 'Un mot pour l’auteur (obligatoire)' : 'Un mot pour l’auteur'} hideLabel={!refusing} value={word} onChangeText={setWord} maxLength={2000} placeholder="Un mot pour l’auteur (facultatif)" />
      <ErrorText>{error}</ErrorText>
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
        {refusing ? (
          <>
            <Button kind="danger" label="Refuser" busy={busy} disabled={!word.trim()} onPress={() => act('refuse')} />
            <Button kind="quiet" label="Annuler" onPress={() => setRefusing(false)} />
          </>
        ) : editing ? (
          <>
            <Button kind="primary" label="Appliquer ma version" busy={busy} onPress={applyEdited} />
            <Button kind="quiet" label="Annuler" onPress={() => setEditing(false)} />
          </>
        ) : (
          <>
            {f.kind === 'proposal' && !f.stale ? <Button kind="primary" label="Appliquer" busy={busy} onPress={() => act('apply')} /> : null}
            {f.kind === 'proposal' && f.element_key && householdId ? <Button label="Modifier puis appliquer" onPress={() => setEditing(true)} /> : null}
            {f.status === 'pending' ? <Button kind={f.kind === 'proposal' ? 'secondary' : 'primary'} label="Publier" busy={busy} onPress={() => act('publish')} /> : null}
            <Button label="Fermer" busy={busy} onPress={() => act('close')} />
            <Button kind="quiet" label="Refuser…" onPress={() => setRefusing(true)} />
          </>
        )}
      </View>
    </View>
  );
}
