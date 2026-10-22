// Les retours de la communauté sur la fiche d'une recette de la communauté : discussions (fils, « +1 »), proposer un
// changement sur une étape, commenter. Un retour attend l'admin avant d'être vu des autres foyers.
import type { Feedback, Ingredient, Phase, Recipe } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { diffWords, formatDuration, searchKey, space } from '@chaudron/domain';
import { useState } from 'react';
import { Pressable, Text, View } from 'react-native';

import { api } from '../lib/api';
import { useTheme } from '../lib/theme';
import { useLoad } from '../lib/useLoad';
import { Body, Button, Card, ErrorText, Field, Pill, Row, Tag, Title } from './index';

const kindLabels = { comment: 'Commentaire', proposal: 'Proposition', tried: 'Variante tentée' } as const;
const statusLabels: Record<string, string> = {
  pending: 'En attente de l’admin',
  published: 'Publié',
  applied: 'Appliqué',
  closed: 'Fermé',
  refused: 'Refusé',
};

const when = (iso: string) => new Intl.DateTimeFormat('fr-FR', { day: 'numeric', month: 'short' }).format(new Date(iso));

/** Ce qu'une proposition change : le texte barré et le texte proposé, mot à mot ; un minuteur, une quantité. */
export function ProposedChanges({ changes }: { changes: Feedback['changes'] }) {
  const { palette, fonts, weights } = useTheme();
  if (!changes || typeof changes !== 'object') return null;
  return (
    <View style={{ gap: space[1] }}>
      {Object.entries(changes as Record<string, { from?: unknown; to?: unknown }>).map(([field, c]) => {
        if (field === 'text' || field === 'spoken_text') {
          return (
            <Text key={field} style={{ fontFamily: fonts.text, fontSize: 15, lineHeight: 22, color: palette.text }}>
              {diffWords(String(c.from ?? ''), String(c.to ?? '')).map((piece, i) => (
                <Text
                  key={i}
                  style={
                    piece.kind === 'removed'
                      ? { color: palette.muted, textDecorationLine: 'line-through' }
                      : piece.kind === 'added'
                        ? { backgroundColor: palette.chip, fontWeight: weights.strong }
                        : undefined
                  }
                >
                  {piece.text}
                </Text>
              ))}
            </Text>
          );
        }
        const show = (v: unknown) =>
          v == null ? '—' : field === 'timer_s' && typeof v === 'number' ? formatDuration(Math.max(1, Math.round(v / 60))) : String(v).replace('.', ',');
        return <Body key={field} size={15}>{`${field === 'timer_s' ? 'Minuteur' : 'Quantité'} : ${show(c.from)} → ${show(c.to)}`}</Body>;
      })}
    </View>
  );
}

/** Où vise un retour, en mots : « Étape 2 · La farce ». */
export function whereOf(recipe: Recipe, key: string | null | undefined): string | null {
  if (!key) return null;
  const line = recipe.ingredients.find((l) => l.key === key);
  if (line) return `Ingrédient · ${line.name}`;
  for (const v of recipe.variants) {
    const i = v.phases.findIndex((p) => p.key === key);
    if (i >= 0) return `Étape ${i + 1} · ${v.phases[i].title}`;
    for (const [j, p] of v.phases.entries()) {
      const k = p.steps.findIndex((s) => s.key === key);
      if (k >= 0) return `Étape ${j + 1} · ${p.title} · geste ${k + 1}`;
    }
  }
  return 'Une étape qui a changé depuis';
}

/** La carte « Discussions » d'une recette de la communauté. */
export function Discussion({ householdId, recipe, onChange }: { householdId: string; recipe: Recipe; onChange: () => void }) {
  const [closed, setClosed] = useState(false);
  const [writing, setWriting] = useState<'comment' | 'tried' | null>(null);
  const list = useLoad(
    () => api.GET('/api/v1/households/{id}/recipes/{slug}/feedback', { params: { path: { id: householdId, slug: recipe.slug }, query: { closed } } }),
    [householdId, recipe.slug, closed, recipe.version],
  );
  const reload = () => {
    void list.reload();
    onChange();
  };
  const all = (list.data ?? []).filter((f) => f.visibility === 'community');
  const top = all.filter((f) => !f.parent_id);
  return (
    <Card>
      <Title level={2}>Discussions</Title>
      <Body tone="muted" size={15}>
        Ce qui a marché, ce qui coince, une meilleure façon de faire : l’admin relit chaque retour avant que les autres
        foyers le voient.
      </Body>
      <ErrorText>{list.error}</ErrorText>
      {top.length === 0 && list.data ? <Body tone="muted" size={15}>{closed ? 'Aucune discussion.' : 'Aucune discussion ouverte.'}</Body> : null}
      <View style={{ gap: space[3] }}>
        {top.map((f) => (
          <Thread key={f.id} feedback={f} replies={all.filter((r) => r.parent_id === f.id)} householdId={householdId} recipe={recipe} onChange={reload} />
        ))}
      </View>
      {writing === 'comment' ? <CommentForm householdId={householdId} recipe={recipe} onDone={() => { setWriting(null); reload(); }} /> : null}
      {writing === 'tried' ? <TriedForm householdId={householdId} recipe={recipe} onDone={() => { setWriting(null); reload(); }} /> : null}
      {writing === null ? (
        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
          <Button label="Commenter" onPress={() => setWriting('comment')} />
          <Button kind="quiet" label="J’ai remplacé un ingrédient" onPress={() => setWriting('tried')} />
          <Button kind="quiet" label={closed ? 'Cacher les discussions fermées' : 'Voir les discussions fermées'} onPress={() => setClosed(!closed)} />
        </View>
      ) : null}
    </Card>
  );
}

function Thread({ feedback: f, replies, householdId, recipe, onChange }: { feedback: Feedback; replies: Feedback[]; householdId: string; recipe: Recipe; onChange: () => void }) {
  const { palette, radius } = useTheme();
  const [replying, setReplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const call = async (run: () => Promise<{ error?: unknown; response: Response }>) => {
    const { response, error: failure } = await run();
    if (!response.ok) return setError(errorMessage(failure));
    onChange();
  };
  const where = whereOf(recipe, f.element_key);
  const tried = f.tried as { replaced?: string; by?: string; verdict?: string } | null;
  return (
    <View style={{ gap: space[2], padding: space[3], borderRadius: radius.button, borderWidth: 1, borderColor: palette.line }}>
      <Row gap={space[2]}>
        <Tag tone={f.kind === 'proposal' ? 'strong' : 'neutral'}>{kindLabels[f.kind]}</Tag>
        {f.status !== 'published' ? <Tag tone={f.status === 'refused' ? 'warn' : 'neutral'}>{statusLabels[f.status] ?? f.status}</Tag> : null}
        {where ? <Body tone="muted" size={14}>{where}</Body> : null}
      </Row>
      {f.kind === 'proposal' ? <ProposedChanges changes={f.changes} /> : null}
      {tried ? <Body size={15}>{`${tried.by} au lieu de ${tried.replaced} : ${tried.verdict === 'good' ? 'bien' : 'pas terrible'}`}</Body> : null}
      <Body size={15}>{f.body}</Body>
      <Body tone="muted" size={13}>{`${f.author_name}${f.household_name ? ` (${f.household_name})` : ''} · ${when(f.created_at)}${f.supports ? ` · +${f.supports}` : ''}`}</Body>
      {f.review_message ? <Body size={14}>{`L’admin : « ${f.review_message} »`}</Body> : null}
      {replies.map((r) => (
        <View key={r.id} style={{ marginLeft: space[4], paddingLeft: space[3], borderLeftWidth: 2, borderLeftColor: palette.line, gap: 2 }}>
          <Body size={15}>{r.body}</Body>
          <Body tone="muted" size={13}>{`${r.author_name}${r.household_name ? ` (${r.household_name})` : ''} · ${when(r.created_at)}${r.status === 'pending' ? ' · en attente de l’admin' : ''}`}</Body>
        </View>
      ))}
      <ErrorText>{error}</ErrorText>
      {replying ? (
        <ReplyForm householdId={householdId} feedbackId={f.id} onDone={() => { setReplying(false); onChange(); }} />
      ) : (
        <Row gap={space[3]}>
          {f.status === 'published' || f.status === 'pending' ? (
            <Pressable onPress={() => setReplying(true)} accessibilityRole="button">
              <Body tone="muted" size={14} style={{ textDecorationLine: 'underline' }}>Répondre</Body>
            </Pressable>
          ) : null}
          {f.status === 'published' && !f.mine ? (
            <Pill
              compact
              label={f.supported ? 'Pareil chez nous ✓' : 'Pareil chez nous'}
              selected={f.supported}
              onPress={() =>
                call(() =>
                  f.supported
                    ? api.DELETE('/api/v1/households/{id}/feedback/{feedback_id}/support', { params: { path: { id: householdId, feedback_id: f.id } } })
                    : api.POST('/api/v1/households/{id}/feedback/{feedback_id}/support', { params: { path: { id: householdId, feedback_id: f.id } } }),
                )
              }
            />
          ) : null}
          {f.status === 'refused' && f.mine ? (
            <Pressable
              onPress={() => call(() => api.PATCH('/api/v1/households/{id}/feedback/{feedback_id}', { params: { path: { id: householdId, feedback_id: f.id } }, body: { visibility: 'household' } }))}
              accessibilityRole="button"
            >
              <Body tone="muted" size={14} style={{ textDecorationLine: 'underline' }}>Le garder comme note du cahier</Body>
            </Pressable>
          ) : null}
        </Row>
      )}
    </View>
  );
}

function ReplyForm({ householdId, feedbackId, onDone }: { householdId: string; feedbackId: string; onDone: () => void }) {
  const [text, setText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const send = async () => {
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/feedback/{feedback_id}/replies', {
      params: { path: { id: householdId, feedback_id: feedbackId } },
      body: { body: text },
    });
    if (!data) return setError(errorMessage(failure));
    onDone();
  };
  return (
    <View style={{ gap: space[2] }}>
      <Field label="Ta réponse" hideLabel value={text} onChangeText={setText} maxLength={2000} placeholder="Ta réponse" />
      <ErrorText>{error}</ErrorText>
      <Row>
        <Button kind="primary" label="Répondre" disabled={!text.trim()} onPress={send} />
        <Button kind="quiet" label="Annuler" onPress={onDone} />
      </Row>
    </View>
  );
}

/** Un commentaire pour la communauté, sur la recette ou une étape. */
export function CommentForm({ householdId, recipe, elementKey, onDone }: { householdId: string; recipe: Recipe; elementKey?: string; onDone: () => void }) {
  const [text, setText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const send = async () => {
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/recipes/{slug}/feedback', {
      params: { path: { id: householdId, slug: recipe.slug } },
      body: { body: text, element_key: elementKey ?? null, visibility: 'community', kind: 'comment' },
    });
    if (!data) return setError(errorMessage(failure));
    onDone();
  };
  return (
    <View style={{ gap: space[2] }}>
      <Field
        label="Commentaire pour la communauté"
        hint="L’admin le relit avant que les autres foyers le voient."
        value={text}
        onChangeText={setText}
        multiline
        maxLength={2000}
        style={{ minHeight: 72, paddingTop: 12, textAlignVertical: 'top' }}
        placeholder="Ça a débordé du Cookeo à 8 portions."
      />
      <ErrorText>{error}</ErrorText>
      <Row>
        <Button kind="primary" label="Envoyer" disabled={!text.trim()} onPress={send} />
        <Button kind="quiet" label="Annuler" onPress={onDone} />
      </Row>
    </View>
  );
}

/** Proposer un changement sur une étape : le texte barré, le texte proposé, le minuteur, et le pourquoi. */
export function ProposalForm({ householdId, recipe, phase, onDone }: { householdId: string; recipe: Recipe; phase: Phase; onDone: () => void }) {
  const [text, setText] = useState(phase.text);
  const [minutes, setMinutes] = useState(phase.timer_s ? String(Math.round(phase.timer_s / 60)) : '');
  const [why, setWhy] = useState('');
  const [error, setError] = useState<string | null>(null);
  const timer = Math.round(Number(minutes.replace(',', '.')) * 60);
  const changes: Record<string, unknown> = {};
  if (text.trim() && text.trim() !== phase.text) changes.text = text.trim();
  if (phase.timer_s && timer > 0 && timer !== phase.timer_s) changes.timer_s = timer;
  const send = async () => {
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/recipes/{slug}/feedback', {
      params: { path: { id: householdId, slug: recipe.slug } },
      body: { body: why, element_key: phase.key, visibility: 'community', kind: 'proposal', changes: changes as never },
    });
    if (!data) return setError(errorMessage(failure));
    onDone();
  };
  return (
    <View style={{ gap: space[2] }}>
      <Field label="Le texte proposé" value={text} onChangeText={setText} multiline style={{ minHeight: 96, paddingTop: 12, textAlignVertical: 'top' }} />
      {phase.timer_s ? <Field label="Minuteur (minutes)" value={minutes} onChangeText={setMinutes} keyboardType="decimal-pad" /> : null}
      {Object.keys(changes).length ? <ProposedChanges changes={Object.fromEntries(Object.entries(changes).map(([k, v]) => [k, { from: k === 'text' ? phase.text : phase.timer_s, to: v }])) as never} /> : null}
      <Field label="Pourquoi" value={why} onChangeText={setWhy} maxLength={2000} placeholder="Le cœur des pommes de terre restait ferme." />
      <ErrorText>{error}</ErrorText>
      <Row>
        <Button kind="primary" label="Proposer" disabled={!why.trim() || !Object.keys(changes).length} onPress={send} />
        <Button kind="quiet" label="Annuler" onPress={onDone} />
      </Row>
    </View>
  );
}

/** « J'ai remplacé un ingrédient » : lequel, par quoi (dans le référentiel), et si c'était bien. */
function TriedForm({ householdId, recipe, onDone }: { householdId: string; recipe: Recipe; onDone: () => void }) {
  const catalog = useLoad(() => api.GET('/api/v1/ingredients'), []);
  const [replaced, setReplaced] = useState<string | null>(null);
  const [by, setBy] = useState<Ingredient | null>(null);
  const [search, setSearch] = useState('');
  const [verdict, setVerdict] = useState<'good' | 'bad'>('good');
  const [text, setText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const needle = searchKey(search);
  const matches = needle ? (catalog.data ?? []).filter((i) => searchKey(i.name).includes(needle)).slice(0, 6) : [];
  const send = async () => {
    if (!replaced || !by) return;
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/recipes/{slug}/feedback', {
      params: { path: { id: householdId, slug: recipe.slug } },
      body: { body: text || `${by.name} au lieu de ${recipe.ingredients.find((l) => l.ingredient === replaced)?.name ?? replaced}.`, visibility: 'community', kind: 'tried', tried: { replaced, by: by.code, verdict } },
    });
    if (!data) return setError(errorMessage(failure));
    onDone();
  };
  return (
    <View style={{ gap: space[3] }}>
      <Body size={15}>Quel ingrédient as-tu remplacé ?</Body>
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
        {recipe.ingredients.map((l) => (
          <Pill key={l.key} compact role="radio" label={l.name} selected={replaced === l.ingredient} onPress={() => setReplaced(l.ingredient)} />
        ))}
      </View>
      <Field label="Par quoi ?" value={by ? by.name : search} onChangeText={(t) => { setBy(null); setSearch(t); }} placeholder="Dinde, lieu noir…" />
      {!by && matches.length ? (
        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
          {matches.map((i) => (
            <Pill key={i.code} compact label={i.name} selected={false} onPress={() => setBy(i)} />
          ))}
        </View>
      ) : null}
      <View style={{ flexDirection: 'row', gap: space[2] }}>
        <Pill role="radio" label="C’était bien" selected={verdict === 'good'} onPress={() => setVerdict('good')} />
        <Pill role="radio" label="Pas terrible" selected={verdict === 'bad'} onPress={() => setVerdict('bad')} />
      </View>
      <Field label="Un mot" value={text} onChangeText={setText} maxLength={2000} placeholder="Super avec de la dinde, même quantité." />
      <ErrorText>{error}</ErrorText>
      <Row>
        <Button kind="primary" label="Envoyer" disabled={!replaced || !by} onPress={send} />
        <Button kind="quiet" label="Annuler" onPress={onDone} />
      </Row>
    </View>
  );
}
