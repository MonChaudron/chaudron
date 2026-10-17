// Composants de l'appli. Couleurs, polices et arrondis viennent du thème choisi (jour ou nuit), espacements de
// @chaudron/domain. Mobile d'abord ; à partir de DESKTOP_WIDTH, mise en page de bureau.
import { space, textSize, touch } from '@chaudron/domain';
import { Link, type Href, usePathname } from 'expo-router';
import type { ReactNode } from 'react';
import {
  ActivityIndicator,
  Platform,
  Pressable,
  ScrollView,
  Text,
  TextInput,
  type TextInputProps,
  type TextStyle,
  useWindowDimensions,
  View,
  type ViewStyle,
} from 'react-native';
import Svg, { Circle } from 'react-native-svg';

import { useTheme } from '../lib/theme';

// Mise en page

/** Largeur à partir de laquelle l'appli passe en mise en page de bureau (barre latérale, colonnes). */
export const DESKTOP_WIDTH = 960;

export function useWide(): boolean {
  const { width } = useWindowDimensions();
  return width >= DESKTOP_WIDTH;
}

/**
 * Page. Téléphone : colonne de 30 rem au plus, en-tête avec le logo. Bureau : barre latérale de navigation et contenu
 * sur toute la largeur utile (les écrans se découpent en colonnes avec `Columns`).
 */
export function Screen({ children, header = true }: { children: ReactNode; header?: boolean }) {
  const { palette } = useTheme();
  const wide = useWide();
  if (wide && header) {
    return (
      <View style={{ flex: 1, flexDirection: 'row', backgroundColor: palette.bg }}>
        <Sidebar />
        <ScrollView style={{ flex: 1 }} contentContainerStyle={{ paddingHorizontal: space[7], paddingVertical: space[6] }} keyboardShouldPersistTaps="handled">
          <View style={{ width: '100%', maxWidth: 1120, gap: space[5] }}>{children}</View>
        </ScrollView>
      </View>
    );
  }
  return (
    <View style={{ flex: 1, backgroundColor: palette.bg }}>
      <ScrollView
        style={{ flex: 1 }}
        contentContainerStyle={{ alignItems: 'center', paddingBottom: space[7] }}
        keyboardShouldPersistTaps="handled"
      >
        <View style={{ width: '100%', maxWidth: 480, paddingHorizontal: space[4], paddingTop: space[5], gap: space[5] }}>
          {header ? <Header /> : null}
          {children}
        </View>
      </ScrollView>
      {header ? <TabBar /> : null}
    </View>
  );
}

/** Téléphone : les grandes sections en bas de l'écran, à portée de pouce. */
function TabBar() {
  const { palette, fonts, weights, radius } = useTheme();
  const path = usePathname();
  return (
    <View
      accessibilityRole="tablist"
      style={{ flexDirection: 'row', gap: space[2], paddingHorizontal: space[3], paddingVertical: space[2], borderTopWidth: 1, borderTopColor: palette.line, backgroundColor: palette.card }}
    >
      {tabs.map((tab) => {
        const active = tab.match(path);
        return (
          <Link
            key={tab.label}
            href={tab.href}
            accessibilityRole="tab"
            aria-selected={active}
            style={{
              flex: 1,
              minHeight: 48,
              lineHeight: 48,
              textAlign: 'center',
              borderRadius: radius.button,
              backgroundColor: active ? palette.tagBg : 'transparent',
              color: active ? palette.tagText : palette.muted,
              fontFamily: fonts.text,
              fontWeight: weights.medium,
              fontSize: 16,
              textDecorationLine: 'none',
            }}
          >
            {tab.label}
          </Link>
        );
      })}
    </View>
  );
}

function Header() {
  const { palette, fonts, weights } = useTheme();
  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' }}>
      <Link href="/" accessibilityLabel="Chaudron, accueil">
        <Logo size={24} />
      </Link>
      <Link href="/compte" style={{ color: palette.muted, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 16, padding: space[2] }}>
        Compte
      </Link>
    </View>
  );
}

type NavEntry = { href: Href; label: string; match: (path: string) => boolean };

const navigation: NavEntry[] = [
  { href: '/', label: 'Cette semaine', match: (p) => p === '/' },
  { href: '/menus', label: 'Menus', match: (p) => p.startsWith('/menus') },
  { href: '/recettes', label: 'Recettes', match: (p) => p.startsWith('/recettes') },
  { href: '/foyer', label: 'Le foyer', match: (p) => p === '/foyer' || p.startsWith('/foyer/membres') || p === '/foyer/acces' },
  { href: '/foyer/equipements', label: 'Le matériel', match: (p) => p === '/foyer/equipements' },
];
/** Téléphone : le matériel est dans le foyer. */
const tabs: NavEntry[] = [
  { href: '/', label: 'Semaine', match: (p) => p === '/' },
  { href: '/menus', label: 'Menus', match: (p) => p.startsWith('/menus') },
  { href: '/recettes', label: 'Recettes', match: (p) => p.startsWith('/recettes') },
  { href: '/foyer', label: 'Foyer', match: (p) => p.startsWith('/foyer') },
];
/** La suite de la V1, montrée pour donner la structure de l'appli. */
const upcoming = ['Courses'];

/** Bureau : logo, navigation, puis le compte en bas. */
function Sidebar() {
  const { palette, fonts, weights, radius } = useTheme();
  const path = usePathname();
  const item = (active: boolean) => ({
    paddingVertical: space[3],
    paddingHorizontal: space[4],
    borderRadius: radius.button,
    backgroundColor: active ? palette.tagBg : 'transparent',
    color: active ? palette.tagText : palette.text,
    fontFamily: fonts.text,
    fontWeight: weights.medium,
    fontSize: 17,
    textDecorationLine: 'none' as const,
  });
  return (
    <View style={{ width: 264, paddingVertical: space[6], paddingHorizontal: space[4], gap: space[6], borderRightWidth: 1, borderRightColor: palette.line, backgroundColor: palette.card }}>
      <Link href="/" accessibilityLabel="Chaudron, accueil" style={{ paddingHorizontal: space[4] }}>
        <Logo size={28} />
      </Link>
      <View accessibilityRole="menu" style={{ gap: space[1] }}>
        {navigation.map((entry) => (
          <Link key={entry.label} href={entry.href} style={item(entry.match(path))}>
            {entry.label}
          </Link>
        ))}
        <Text style={{ marginTop: space[5], marginBottom: space[1], paddingHorizontal: space[4], fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 13, letterSpacing: 1, textTransform: 'uppercase', color: palette.muted }}>
          Bientôt
        </Text>
        {upcoming.map((label) => (
          <Text key={label} style={{ ...item(false), color: palette.muted, opacity: 0.7 }}>
            {label}
          </Text>
        ))}
      </View>
      <View style={{ marginTop: 'auto' }}>
        <Link href="/compte" style={item(path === '/compte')}>
          Compte
        </Link>
      </View>
    </View>
  );
}

/**
 * Colonnes sur bureau, empilées sur téléphone. `weights` : part de largeur de chaque colonne (ex. [3, 2]).
 * Chaque enfant direct est une colonne.
 */
export function Columns({ children, weights = [], gap = space[5] }: { children: ReactNode[]; weights?: number[]; gap?: number }) {
  const wide = useWide();
  if (!wide) return <View style={{ gap }}>{children}</View>;
  return (
    <View style={{ flexDirection: 'row', alignItems: 'flex-start', gap }}>
      {children.map((child, i) => (
        <View key={i} style={{ flex: weights[i] ?? 1, minWidth: 0, gap }}>
          {child}
        </View>
      ))}
    </View>
  );
}

/**
 * Connexion et invitation. Téléphone : logo puis la carte. Bureau : écran partagé, présentation de Chaudron à gauche,
 * la carte à droite.
 */
export function AuthLayout({ children }: { children: ReactNode }) {
  const { palette, fonts, weights } = useTheme();
  const wide = useWide();
  if (!wide) {
    return (
      <Screen header={false}>
        <View style={{ alignItems: 'center', paddingTop: space[6], paddingBottom: space[2] }}>
          <Logo size={48} />
        </View>
        {children}
      </Screen>
    );
  }
  const points = [
    'Des recettes qui s’adaptent à ton matériel : Cookeo, four, Thermomix.',
    'Les menus de la semaine, le batch la veille, les portions de chacun.',
    'La liste de courses partagée, cochée en direct à plusieurs.',
  ];
  return (
    <View style={{ flex: 1, flexDirection: 'row', backgroundColor: palette.bg }}>
      <View style={{ flex: 1, justifyContent: 'center', paddingHorizontal: 80, paddingVertical: space[7], gap: space[6], backgroundColor: palette.card }}>
        <Logo size={64} />
        <Text style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: 48, lineHeight: 56, color: palette.text, maxWidth: 560 }}>
          Un plat, plusieurs assiettes.
        </Text>
        <View style={{ gap: space[4], maxWidth: 520 }}>
          {points.map((point) => (
            <View key={point} style={{ flexDirection: 'row', gap: space[3], alignItems: 'flex-start' }}>
              <View style={{ width: 10, height: 10, borderRadius: 5, marginTop: 9, backgroundColor: palette.logoO }} />
              <Text style={{ flex: 1, fontFamily: fonts.text, fontSize: 19, lineHeight: 28, color: palette.muted }}>{point}</Text>
            </View>
          ))}
        </View>
      </View>
      <ScrollView style={{ flex: 1 }} contentContainerStyle={{ flexGrow: 1, justifyContent: 'center', alignItems: 'center', padding: space[7] }}>
        <View style={{ width: '100%', maxWidth: 460 }}>{children}</View>
      </ScrollView>
    </View>
  );
}

/** Retour vers l'écran parent. */
export function Back({ href, label }: { href: Href; label: string }) {
  const { palette, fonts, weights } = useTheme();
  return (
    <Link href={href} style={{ color: palette.link, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 16, paddingVertical: space[2] }}>
      ← {label}
    </Link>
  );
}

export function Card({ children, style }: { children: ReactNode; style?: ViewStyle }) {
  const { palette, radius } = useTheme();
  return (
    <View style={[{ backgroundColor: palette.card, borderRadius: radius.card, padding: space[5], gap: space[4] }, style]}>
      {children}
    </View>
  );
}

export function Stack({ children, gap = space[3], style }: { children: ReactNode; gap?: number; style?: ViewStyle }) {
  return <View style={[{ gap }, style]}>{children}</View>;
}

export function Row({ children, gap = space[2], style }: { children: ReactNode; gap?: number; style?: ViewStyle }) {
  return <View style={[{ flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap }, style]}>{children}</View>;
}

// Logo

/**
 * « chaudron » dans la police de titre du thème ; le o est le dessin de l'icône (anneau et deux bulles), calé sur la
 * hauteur des minuscules et posé sur la ligne de base. Web : le SVG est en ligne dans le texte.
 */
export function Logo({ size = 24 }: { size?: number }) {
  const { palette, fonts, weights, theme } = useTheme();
  const height = size * theme.xHeight;
  return (
    <Text
      accessibilityRole="header"
      aria-label="Chaudron"
      style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: size, lineHeight: size * 1.3, color: palette.text }}
    >
      chaudr
      <Svg
        viewBox="13 6 38 51"
        width={height}
        height={(height * 51) / 38}
        style={{ marginHorizontal: size * 0.03 } as never}
        aria-hidden
      >
        <Circle cx={32} cy={38} r={15} fill="none" stroke={palette.logoO} strokeWidth={8} />
        <Circle cx={26} cy={14} r={4} fill={palette.logoO} />
        <Circle cx={37} cy={9} r={3} fill={palette.logoO} />
      </Svg>
      n
    </Text>
  );
}

// Texte

export function Title({ children, level = 1 }: { children: ReactNode; level?: 1 | 2 }) {
  const { palette, fonts, weights } = useTheme();
  const wide = useWide();
  const size = level === 1 ? (wide ? 40 : textSize.h1) : textSize.h2;
  return (
    <Text
      role="heading"
      aria-level={level}
      style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: size, lineHeight: size * 1.18, color: palette.text }}
    >
      {children}
    </Text>
  );
}

type Tone = 'normal' | 'muted' | 'alert';

export function Body({ children, tone = 'normal', size = textSize.body, style }: { children: ReactNode; tone?: Tone; size?: number; style?: TextStyle }) {
  const { palette, fonts, weights } = useTheme();
  const color = tone === 'muted' ? palette.muted : tone === 'alert' ? palette.alert : palette.text;
  return (
    <Text style={[{ fontFamily: fonts.text, fontSize: size, lineHeight: size * 1.5, color }, tone === 'alert' && { fontWeight: weights.medium }, style]}>
      {children}
    </Text>
  );
}

/** Gras du texte, selon la police du thème (800 en Atkinson : à 700, il se distingue mal du demi-gras de loin). */
export function Strong({ children }: { children: ReactNode }) {
  const { weights } = useTheme();
  return <Text style={{ fontWeight: weights.strong }}>{children}</Text>;
}

export function ErrorText({ children }: { children: ReactNode }) {
  if (!children) return null;
  return (
    <Body tone="alert" size={16}>
      {children}
    </Body>
  );
}

export function Tag({ children }: { children: ReactNode }) {
  const { palette, fonts, radius, weights } = useTheme();
  return (
    <View style={{ alignSelf: 'flex-start', backgroundColor: palette.tagBg, borderRadius: radius.tag, paddingVertical: 6, paddingHorizontal: 12 }}>
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: textSize.small, color: palette.tagText }}>{children}</Text>
    </View>
  );
}

// Actions

type ButtonKind = 'primary' | 'secondary' | 'quiet' | 'danger';

export function Button({
  label,
  onPress,
  kind = 'secondary',
  busy = false,
  disabled = false,
  icon,
  accessibilityLabel,
}: {
  label: string;
  onPress: () => void;
  kind?: ButtonKind;
  busy?: boolean;
  disabled?: boolean;
  icon?: ReactNode;
  accessibilityLabel?: string;
}) {
  const { palette, fonts, radius, weights } = useTheme();
  const styles: Record<ButtonKind, { bg: string; fg: string; border: string; borderWidth: number }> = {
    primary: { bg: palette.action, fg: palette.onAction, border: palette.actionBorder, borderWidth: 2 },
    secondary: { bg: palette.card, fg: palette.text, border: palette.line, borderWidth: 1.5 },
    quiet: { bg: 'transparent', fg: palette.muted, border: 'transparent', borderWidth: 0 },
    danger: { bg: 'transparent', fg: palette.alert, border: palette.line, borderWidth: 1.5 },
  };
  const s = styles[kind];
  return (
    <Pressable
      onPress={onPress}
      disabled={disabled || busy}
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel ?? label}
      accessibilityState={{ disabled: disabled || busy, busy }}
      style={({ pressed, hovered }: { pressed: boolean; hovered?: boolean }) => ({
        minHeight: kind === 'quiet' ? 44 : touch,
        paddingHorizontal: space[5],
        borderRadius: radius.button,
        backgroundColor: s.bg,
        borderColor: hovered && kind === 'secondary' ? palette.muted : s.border,
        borderWidth: s.borderWidth,
        flexDirection: 'row',
        alignItems: 'center',
        justifyContent: 'center',
        gap: space[2],
        opacity: disabled ? 0.5 : pressed ? 0.85 : 1,
      })}
    >
      {busy ? <ActivityIndicator color={s.fg} /> : icon}
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: textSize.button, color: s.fg }}>{label}</Text>
    </Pressable>
  );
}

/** Lien vers un écran de l'appli, présenté comme un bouton. */
export function LinkButton({ label, href, kind = 'secondary' }: { label: string; href: Href; kind?: Exclude<ButtonKind, 'danger'> }) {
  const { palette, fonts, radius, weights } = useTheme();
  const primary = kind === 'primary';
  return (
    <Link
      href={href}
      style={{
        minHeight: touch,
        paddingHorizontal: space[5],
        borderRadius: radius.button,
        backgroundColor: primary ? palette.action : palette.card,
        borderColor: primary ? palette.actionBorder : palette.line,
        borderWidth: primary ? 2 : 1.5,
        color: primary ? palette.onAction : palette.text,
        fontFamily: fonts.text,
        fontWeight: weights.medium,
        fontSize: textSize.button,
        textAlign: 'center',
        lineHeight: touch - 4,
        textDecorationLine: 'none',
      }}
    >
      {label}
    </Link>
  );
}

/** Ligne cliquable d'une liste (membre, compte…), avec un chevron. */
export function ListRow({ title, detail, extra, onPress, last }: { title: string; detail?: string; extra?: ReactNode; onPress?: () => void; last?: boolean }) {
  const { palette, fonts, weights } = useTheme();
  const content = (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: space[3], paddingVertical: space[3], borderBottomWidth: last ? 0 : 1, borderBottomColor: palette.line }}>
      <View style={{ flex: 1, gap: 2 }}>
        <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 17, color: palette.text }}>{title}</Text>
        {detail ? <Body tone="muted" size={15}>{detail}</Body> : null}
        {extra}
      </View>
      {onPress ? <Text style={{ color: palette.muted, fontSize: 22 }} aria-hidden>›</Text> : null}
    </View>
  );
  if (!onPress) return content;
  return (
    <Pressable onPress={onPress} accessibilityRole="button" accessibilityLabel={title} style={({ pressed }) => ({ opacity: pressed ? 0.7 : 1 })}>
      {content}
    </Pressable>
  );
}

// Saisie

/** Champ de saisie. `hideLabel` : le libellé n'est lu que par les lecteurs d'écran (champ dans une ligne). */
export function Field({ label, hint, hideLabel = false, ...props }: TextInputProps & { label: string; hint?: string; hideLabel?: boolean }) {
  const { palette, fonts, radius, weights } = useTheme();
  return (
    <View style={{ gap: space[2] }}>
      {hideLabel ? null : <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: textSize.small, color: palette.text }}>{label}</Text>}
      <TextInput
        accessibilityLabel={label}
        placeholderTextColor={palette.muted}
        {...props}
        style={[
          {
            minHeight: touch,
            paddingHorizontal: space[4],
            borderWidth: 1.5,
            borderColor: palette.line,
            borderRadius: radius.button,
            backgroundColor: palette.card,
            color: palette.text,
            fontFamily: fonts.text,
            fontSize: textSize.button,
          },
          Platform.OS === 'web' ? ({ outlineColor: palette.link } as object) : null,
          props.style,
        ]}
      />
      {hint ? <Body tone="muted" size={14}>{hint}</Body> : null}
    </View>
  );
}

/** Choix parmi quelques options, en pastilles. */
export function Choice<T extends string>({ options, value, onChange, label }: { options: { value: T; label: string }[]; value: T; onChange: (value: T) => void; label: string }) {
  return (
    <View accessibilityRole="radiogroup" aria-label={label} style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[2] }}>
      {options.map((option) => (
        <Pill key={option.value} label={option.label} selected={option.value === value} onPress={() => onChange(option.value)} role="radio" />
      ))}
    </View>
  );
}

/** Pastille sélectionnable (équipement, choix, jour de présence). */
export function Pill({ label, selected, onPress, role = 'checkbox', tone = 'normal', compact = false, accessibilityLabel }: { label: string; selected: boolean; onPress: () => void; role?: 'checkbox' | 'radio'; tone?: 'normal' | 'alert'; compact?: boolean; accessibilityLabel?: string }) {
  const { palette, fonts, radius, weights } = useTheme();
  const on = selected;
  const fg = on ? (tone === 'alert' ? palette.card : palette.tagText) : palette.muted;
  const bg = on ? (tone === 'alert' ? palette.alert : palette.tagBg) : 'transparent';
  return (
    <Pressable
      onPress={onPress}
      accessibilityRole={role}
      accessibilityState={{ checked: on }}
      accessibilityLabel={accessibilityLabel ?? label}
      style={({ pressed }) => ({
        minHeight: compact ? 44 : 44,
        minWidth: compact ? 44 : undefined,
        paddingHorizontal: compact ? 0 : 14,
        borderRadius: radius.tag,
        backgroundColor: bg,
        borderWidth: 1.5,
        borderColor: on ? bg : palette.line,
        alignItems: 'center',
        justifyContent: 'center',
        opacity: pressed ? 0.8 : 1,
      })}
    >
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: textSize.small, color: fg }}>{label}</Text>
    </Pressable>
  );
}

/** Pendant le chargement d'un écran. */
export function Loading() {
  const { palette } = useTheme();
  return (
    <View style={{ paddingVertical: space[7], alignItems: 'center' }}>
      <ActivityIndicator color={palette.muted} />
    </View>
  );
}
