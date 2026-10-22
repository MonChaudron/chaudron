// Une différence entre deux versions d'une recette : où, quoi, et les textes mot à mot (barré, ajouté).
import { diffWords, fieldLabels, formatDuration, type RecipeChange, space } from '@chaudron/domain';
import type { ReactNode } from 'react';
import { Text, View } from 'react-native';

import { useTheme } from '../lib/theme';
import { Body, Row, Tag } from './index';

const kindLabels: Record<RecipeChange['kind'], string> = {
  added: 'Ajouté',
  removed: 'Retiré',
  changed: 'Modifié',
  moved: 'Déplacé',
};

const TEXT_FIELDS = new Set(['title', 'description', 'text', 'spoken_text', 'note', 'label', 'source']);

function show(value: unknown, field: string): string {
  if (value === null || value === undefined || value === '') return '—';
  if (field === 'timer_s' && typeof value === 'number') return formatDuration(Math.max(1, Math.round(value / 60)));
  if (Array.isArray(value)) return value.length ? value.join(', ') : '—';
  if (typeof value === 'boolean') return value ? 'oui' : 'non';
  return String(value);
}

/** `action` : un bouton sous la différence (« Reprendre »). */
export function ChangeView({ change, action }: { change: RecipeChange; action?: ReactNode }) {
  const { palette, fonts, weights } = useTheme();
  return (
    <View style={{ gap: space[2] }}>
      <Row gap={space[2]}>
        <Tag tone={change.kind === 'removed' ? 'warn' : 'neutral'}>{kindLabels[change.kind]}</Tag>
        <Text style={{ flexShrink: 1, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 16, color: palette.text }}>{change.where}</Text>
      </Row>
      {change.fields.map((f) => (
        <View key={f.field} style={{ gap: 2, paddingLeft: space[3], borderLeftWidth: 2, borderLeftColor: palette.line }}>
          <Body tone="muted" size={14}>{fieldLabels[f.field] ?? f.field}</Body>
          {TEXT_FIELDS.has(f.field) && typeof (f.before ?? '') === 'string' && typeof (f.after ?? '') === 'string' ? (
            <Text style={{ fontFamily: fonts.text, fontSize: 16, lineHeight: 24, color: palette.text }}>
              {diffWords(String(f.before ?? ''), String(f.after ?? '')).map((piece, j) => (
                <Text
                  key={j}
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
          ) : (
            <Body size={16}>{`${show(f.before, f.field)} → ${show(f.after, f.field)}`}</Body>
          )}
        </View>
      ))}
      {action}
    </View>
  );
}
