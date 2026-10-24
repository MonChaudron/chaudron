// Apports d'une portion (design-nutrition.md) : la note d'équilibre, la ligne « ta portion », les poids à servir, et
// le détail qui se déplie en touchant la note.
import type { Grade, Nutrition } from '@chaudron/api';
import { energySplitColors, factorText, type FactorView, formatGrams, formatKcal, formatSalt, gradeColors, gradeLetter, gradeMissingText, portionLine, serveLine, space, weighed } from '@chaudron/domain';
import { type ReactNode, useState } from 'react';
import { Pressable, Text, View } from 'react-native';

import { useTheme } from '../lib/theme';
import { Body, Stack } from './index';

const grades: Grade[] = ['a', 'b', 'c', 'd', 'e'];

/**
 * La note : les cinq rangs côte à côte, l'actif en grand et cerclé ; `compact`, l'actif seul. Sans note : « note
 * indisponible ». Avec `onPress`, elle déplie le détail.
 */
export function GradePill({ grade, open = false, onPress, compact = false }: { grade?: Grade | null; open?: boolean; onPress?: () => void; compact?: boolean }) {
  const { palette, fonts, weights, radius, mode } = useTheme();
  const label = grade ? `Note d’équilibre ${gradeLetter(grade)} sur une échelle de A à E` : 'Note indisponible';
  const content = !grade ? (
    <View style={{ backgroundColor: palette.chip, borderRadius: radius.tag, paddingVertical: 4, paddingHorizontal: 10 }}>
      <Text style={{ fontFamily: fonts.text, fontSize: 14, color: palette.muted }}>note indisponible</Text>
    </View>
  ) : (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 2 }}>
      {(compact ? [grade] : grades).map((g) => {
        const on = g === grade;
        const { bg, fg } = gradeColors[mode][g];
        return (
          <View
            key={g}
            style={{
              width: on ? 28 : 20,
              height: on ? 30 : 24,
              borderRadius: 6,
              backgroundColor: bg,
              opacity: on ? 1 : 0.45,
              borderWidth: on ? 2 : 0,
              borderColor: fg,
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: on ? 15 : 11, color: fg }}>{gradeLetter(g)}</Text>
          </View>
        );
      })}
    </View>
  );
  if (!onPress) {
    return (
      <View accessible accessibilityLabel={label} style={{ alignSelf: 'flex-start' }}>
        {content}
      </View>
    );
  }
  return (
    <Pressable
      onPress={onPress}
      accessibilityRole="button"
      accessibilityLabel={`${label}. ${open ? 'Masquer' : 'Voir'} le détail des apports`}
      accessibilityState={{ expanded: open }}
      style={({ pressed }) => ({ alignSelf: 'flex-start', minHeight: 44, justifyContent: 'center', opacity: pressed ? 0.7 : 1 })}
    >
      {content}
    </Pressable>
  );
}

/** La note (qui déplie le détail), « Ta portion : ~540 kcal · 39 g de protéines », et ce qu'on sert. Rien pour une
 * recette sans rien à peser. */
export function NutritionSummary({ nutrition }: { nutrition: Nutrition }) {
  const { palette, fonts, weights } = useTheme();
  const [open, setOpen] = useState(false);
  if (!weighed(nutrition)) return null;
  const serve = serveLine(nutrition);
  return (
    <Stack gap={space[2]}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: space[3], flexWrap: 'wrap' }}>
        <GradePill grade={nutrition.grade} open={open} onPress={() => setOpen(!open)} />
        <Text style={{ flexShrink: 1, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 15, color: palette.text }}>{portionLine(nutrition)}</Text>
      </View>
      {serve ? <Body tone="muted" size={14.5}>{serve}</Body> : null}
      {open ? <NutritionDetail nutrition={nutrition} /> : null}
    </Stack>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  const { palette, fonts, weights } = useTheme();
  return (
    <Stack gap={space[2]}>
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 13, letterSpacing: 0.4, textTransform: 'uppercase', color: palette.muted }}>{title}</Text>
      {children}
    </Stack>
  );
}

function Tip({ children }: { children: string }) {
  const { palette } = useTheme();
  return (
    <View style={{ backgroundColor: palette.chip, borderRadius: 10, paddingVertical: 6, paddingHorizontal: 10 }}>
      <Body size={13.5}>{children}</Body>
    </View>
  );
}

/** Le détail : d'où viennent les calories du repas, la part de la journée, ce qui fait la note. */
export function NutritionDetail({ nutrition: n }: { nutrition: Nutrition }) {
  const { palette, fonts, mode } = useTheme();
  const colors = energySplitColors[mode];
  const split = [
    { label: 'Protéines', pct: n.energy_split.protein, color: colors.protein },
    { label: 'Glucides', pct: n.energy_split.carbs, color: colors.carbs },
    { label: 'Lipides', pct: n.energy_split.fat, color: colors.fat },
    { label: 'Fibres', pct: n.energy_split.fiber, color: colors.fiber },
  ];
  const a = n.amounts;
  const d = n.daily_pct;
  // `limit` : au-delà de 50 %, en rouge (pas les protéines, les glucides ni les fibres : en avoir beaucoup n'est pas
  // un excès).
  const rows: { label: string; value: string; pct: number; sub?: boolean; limit?: boolean }[] = [
    { label: 'Énergie', value: formatKcal(a.energy_kcal), pct: d.energy_kcal, limit: true },
    { label: 'Protéines', value: formatGrams(a.protein_g), pct: d.protein_g },
    { label: 'Glucides', value: formatGrams(a.carbs_g), pct: d.carbs_g },
    { label: 'dont sucres', value: formatGrams(a.sugars_g), pct: d.sugars_g, sub: true, limit: true },
    { label: 'Lipides', value: formatGrams(a.fat_g), pct: d.fat_g, limit: true },
    { label: 'dont saturés', value: formatGrams(a.sat_fat_g), pct: d.sat_fat_g, sub: true, limit: true },
    { label: 'Fibres', value: formatGrams(a.fiber_g), pct: d.fiber_g },
    { label: 'Sel', value: formatSalt(a.salt_g), pct: d.salt_g, limit: true },
  ];
  const cell = { fontFamily: fonts.text, fontSize: 14.5, color: palette.text, fontVariant: ['tabular-nums' as const] };
  const sentence = (items: FactorView[]) => items.map(factorText).join(', ');
  return (
    <Stack gap={space[4]} style={{ maxWidth: 520 }}>
      <Section title="Le repas : d’où viennent les calories">
        <View style={{ flexDirection: 'row', height: 14, borderRadius: 7, overflow: 'hidden', backgroundColor: palette.chip }}>
          {split.map((s) => (s.pct > 0 ? <View key={s.label} style={{ flex: s.pct, backgroundColor: s.color }} /> : null))}
        </View>
        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[3] }}>
          {split.map((s) => (
            <View key={s.label} style={{ flexDirection: 'row', alignItems: 'center', gap: 4 }}>
              <View style={{ width: 9, height: 9, borderRadius: 5, backgroundColor: s.color }} />
              <Body tone="muted" size={13.5}>{`${s.label} ${s.pct} %`}</Body>
            </View>
          ))}
        </View>
        <Tip>Un repas équilibré tourne autour de 15-25 % de protéines, 40-55 % de glucides, 30-40 % de lipides.</Tip>
      </Section>

      <Section title={n.portion === 'member' ? 'La journée : la part de tes apports' : 'La journée : la part des apports d’un adulte'}>
        <View>
          {rows.map((r) => (
            <View key={r.label} style={{ flexDirection: 'row', alignItems: 'center', gap: space[2], paddingVertical: 5, borderBottomWidth: 1, borderBottomColor: palette.line }}>
              <Text style={[cell, { flex: 1, paddingLeft: r.sub ? space[3] : 0, color: r.sub ? palette.muted : palette.text }]}>{r.label}</Text>
              <Text style={[cell, { minWidth: 76, textAlign: 'right' }]}>{r.value}</Text>
              <View style={{ width: 96, flexDirection: 'row', alignItems: 'center', justifyContent: 'flex-end', gap: 6 }}>
                <View style={{ width: Math.min(r.pct, 100) * 0.5, height: 7, borderRadius: 4, backgroundColor: r.limit && r.pct > 50 ? palette.alert : palette.link }} />
                <Text style={[cell, { minWidth: 40, textAlign: 'right' }]}>{`${r.pct} %`}</Text>
              </View>
            </View>
          ))}
        </View>
        <Tip>Un dîner représente souvent 30 à 40 % de la journée. Au-delà de 50 % sur une ligne, c’est beaucoup pour un seul repas.</Tip>
      </Section>

      <Stack gap={space[2]}>
        {gradeMissingText(n) ? <Body tone="muted" size={14}>{gradeMissingText(n)}</Body> : null}
        {n.lowers.length ? <Body size={14.5}>{`Ce qui baisse la note : ${sentence(n.lowers)}.`}</Body> : null}
        {n.holds.length ? <Body size={14.5}>{`Ce qui la tient : ${sentence(n.holds)}.`}</Body> : null}
        {n.missing.length ? <Body tone="muted" size={14}>{`Sans données, hors du calcul : ${n.missing.join(', ')}.`}</Body> : null}
        {n.to_taste.length ? <Body tone="muted" size={14}>{`Non comptés, au goût : ${n.to_taste.join(', ').toLocaleLowerCase('fr-FR')}.`}</Body> : null}
        <Body tone="muted" size={13}>Estimations calculées selon l’algorithme du Nutri-Score, d’après la table Ciqual de l’Anses. Poids cuits estimés.</Body>
      </Stack>
    </Stack>
  );
}
