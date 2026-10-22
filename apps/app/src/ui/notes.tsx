// Le cahier du foyer sur une recette : notes privées, sur la recette entière ou sur une étape, avec un changement
// éventuel (« chez nous : 14 min ») affiché à côté du texte, sans toucher la recette.
import type { Feedback } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { formatDuration, space } from '@chaudron/domain';
import { useState } from 'react';
import { Pressable, Text, View } from 'react-native';

import { api } from '../lib/api';
import { useTheme } from '../lib/theme';
import { Icon } from './icons';
import { Body, Button, ErrorText, Field, Row } from './index';

const when = (iso: string) => new Intl.DateTimeFormat('fr-FR', { day: 'numeric', month: 'short' }).format(new Date(iso));

/** « chez nous : 14 min », d'après les changements portés par la note. */
export function changeText(changes: Feedback['changes']): string | null {
  if (!changes || typeof changes !== 'object') return null;
  const parts = Object.entries(changes as Record<string, { to?: unknown }>).map(([field, change]) => {
    const to = change?.to;
    if (field === 'timer_s' && typeof to === 'number') return formatDuration(Math.max(1, Math.round(to / 60)));
    if (field === 'qty') return `quantité ${String(to).replace('.', ',')}`;
    return `« ${String(to)} »`;
  });
  return parts.length ? `Chez nous : ${parts.join(', ')}` : null;
}

/** Les notes d'une étape ou de la recette, à fermer quand elles ne servent plus. */
export function Notes({ notes, householdId, onChange }: { notes: Feedback[]; householdId: string; onChange: () => void }) {
  if (!notes.length) return null;
  return (
    <View style={{ gap: space[2] }}>
      {notes.map((note) => (
        <NoteRow key={note.id} note={note} householdId={householdId} onChange={onChange} />
      ))}
    </View>
  );
}

function NoteRow({ note, householdId, onChange }: { note: Feedback; householdId: string; onChange: () => void }) {
  const { palette, fonts, weights, radius } = useTheme();
  const [error, setError] = useState<string | null>(null);
  const change = changeText(note.changes);
  const close = async () => {
    const { data, error: failure } = await api.PATCH('/api/v1/households/{id}/feedback/{feedback_id}', {
      params: { path: { id: householdId, feedback_id: note.id } },
      body: { closed: note.status !== 'closed' },
    });
    if (!data) return setError(errorMessage(failure));
    onChange();
  };
  return (
    <View style={{ flexDirection: 'row', gap: space[2], padding: space[3], borderRadius: radius.button, backgroundColor: palette.chip }}>
      <Icon name="note" size={18} color={palette.muted} />
      <View style={{ flex: 1, gap: 2 }}>
        {change ? <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 15, color: palette.text }}>{change}</Text> : null}
        <Body size={15}>{note.body}</Body>
        <Row gap={space[3]}>
          <Body tone="muted" size={13}>{`${note.author_name} · ${when(note.created_at)}`}</Body>
          <Pressable onPress={close} accessibilityRole="button">
            <Body tone="muted" size={13} style={{ textDecorationLine: 'underline' }}>{note.status === 'closed' ? 'Rouvrir' : 'Fermer'}</Body>
          </Pressable>
        </Row>
        <ErrorText>{error}</ErrorText>
      </View>
    </View>
  );
}

/**
 * Écrire une note : sur la recette (`elementKey` absent) ou sur une étape, avec, sur une étape minutée, « chez nous :
 * N min ».
 */
export function NoteForm({
  householdId,
  slug,
  elementKey,
  timer = false,
  onDone,
}: {
  householdId: string;
  slug: string;
  elementKey?: string;
  timer?: boolean;
  onDone: () => void;
}) {
  const [text, setText] = useState('');
  const [minutes, setMinutes] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const min = Number(minutes.replace(',', '.'));
  const save = async () => {
    setBusy(true);
    const { data, error: failure } = await api.POST('/api/v1/households/{id}/recipes/{slug}/feedback', {
      params: { path: { id: householdId, slug } },
      body: {
        element_key: elementKey ?? null,
        body: text,
        // Le contrat dit « objet » sans le détailler : un champ, sa nouvelle valeur.
        changes: (timer && min > 0 ? { timer_s: Math.round(min * 60) } : null) as never,
      },
    });
    setBusy(false);
    if (!data) return setError(errorMessage(failure));
    setText('');
    setMinutes('');
    onDone();
  };
  return (
    <View style={{ gap: space[2] }}>
      <Field
        label={elementKey ? 'Note sur cette étape' : 'Note sur la recette'}
        hint="Pour le foyer seulement, comme dans un cahier de recettes."
        value={text}
        onChangeText={setText}
        multiline
        maxLength={2000}
        style={{ minHeight: 72, paddingTop: 12, textAlignVertical: 'top' }}
        placeholder={elementKey ? 'Cubes de 2 cm, pas plus.' : 'Les enfants adorent ; doubler l’ail.'}
      />
      {timer ? (
        <Field label="Chez nous, le minuteur (minutes)" value={minutes} onChangeText={setMinutes} keyboardType="decimal-pad" placeholder="14" />
      ) : null}
      <ErrorText>{error}</ErrorText>
      <Row>
        <Button kind="primary" label="Noter" busy={busy} disabled={!text.trim()} onPress={save} />
        <Button kind="quiet" label="Annuler" onPress={onDone} />
      </Row>
    </View>
  );
}

const ratingLabels = { top: 'Top', bof: 'Bof', flop: 'Raté' } as const;
type RatingValue = keyof typeof ratingLabels;

/** « Comment c'était ? » après un repas du menu ; le mot devient une note de la recette. */
export function RateMeal({
  householdId,
  slug,
  date,
  meal,
  current,
  onDone,
}: {
  householdId: string;
  slug: string;
  date: string;
  meal: 'lunch' | 'dinner';
  current: RatingValue | null | undefined;
  onDone: () => void;
}) {
  const { palette, fonts, weights, radius } = useTheme();
  const [rating, setRating] = useState<RatingValue | null>(current ?? null);
  const [word, setWord] = useState('');
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const send = async (value: RatingValue, text?: string) => {
    setRating(value);
    const { response, error: failure } = await api.PUT('/api/v1/households/{id}/ratings', {
      params: { path: { id: householdId } },
      body: { recipe: slug, date, meal, rating: value, word: text?.trim() || null },
    });
    if (!response.ok) return setError(errorMessage(failure));
    setSaved(true);
    if (text) setWord('');
    onDone();
  };
  return (
    <View style={{ gap: space[3] }}>
      <View accessibilityRole="radiogroup" aria-label="Comment c’était ?" style={{ flexDirection: 'row', gap: space[2] }}>
        {(Object.keys(ratingLabels) as RatingValue[]).map((value) => {
          const on = rating === value;
          return (
            <Pressable
              key={value}
              accessibilityRole="radio"
              accessibilityState={{ selected: on }}
              onPress={() => send(value)}
              style={{
                minHeight: 48,
                paddingHorizontal: space[5],
                justifyContent: 'center',
                borderRadius: radius.button,
                borderWidth: 1.5,
                borderColor: on ? palette.tagBg : palette.line,
                backgroundColor: on ? palette.tagBg : palette.card,
              }}
            >
              <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 17, color: on ? palette.tagText : palette.text }}>{ratingLabels[value]}</Text>
            </Pressable>
          );
        })}
      </View>
      {rating ? (
        <Row gap={space[2]} style={{ alignItems: 'flex-end' }}>
          <View style={{ flex: 1, minWidth: 200 }}>
            <Field label="Un mot ?" hideLabel value={word} onChangeText={setWord} maxLength={2000} placeholder="Un mot ? « Super avec de la dinde »" />
          </View>
          <Button label="Garder" disabled={!word.trim()} onPress={() => send(rating, word)} />
        </Row>
      ) : null}
      {saved ? <Body tone="muted" size={14}>C’est noté, merci.</Body> : null}
      <ErrorText>{error}</ErrorText>
    </View>
  );
}
