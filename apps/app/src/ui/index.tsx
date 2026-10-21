// Composants de l'appli. Couleurs, polices et arrondis viennent du thème choisi (jour ou nuit), espacements de
// @chaudron/domain. Mobile d'abord ; à partir de DESKTOP_WIDTH, mise en page de bureau.
import { type Hue, type Pictogram, space, splitTitle, textSize, touch, vignettes } from '@chaudron/domain';
import { Link, type Href, usePathname } from 'expo-router';
import { type ReactNode, useState } from 'react';
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

import { useSession } from '../lib/session';
import { useTheme } from '../lib/theme';
import { Icon, type IconName } from './icons';

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

/** Téléphone : les grandes sections en bas de l'écran, à portée de pouce ; l'icône de l'onglet ouvert sur l'accent. */
function TabBar() {
  const { palette, fonts, weights } = useTheme();
  const path = usePathname();
  return (
    <View
      accessibilityRole="tablist"
      style={{ flexDirection: 'row', paddingHorizontal: space[2], paddingVertical: space[2], borderTopWidth: 1, borderTopColor: palette.line, backgroundColor: palette.card }}
    >
      {tabs.map((tab) => {
        const active = tab.match(path);
        return (
          <Link key={tab.label} href={tab.href} accessibilityRole="tab" aria-selected={active} style={{ flex: 1, textDecorationLine: 'none' }}>
            <View style={{ alignItems: 'center', gap: 3, minHeight: 48, justifyContent: 'center' }}>
              <View style={{ paddingHorizontal: 16, paddingVertical: 4, borderRadius: 999, backgroundColor: active ? palette.action : 'transparent' }}>
                <Icon name={tab.icon} size={22} color={active ? palette.onAction : palette.muted} />
              </View>
              <Text style={{ fontFamily: fonts.text, fontWeight: active ? weights.strong : weights.medium, fontSize: 12.5, color: active ? palette.text : palette.muted }}>{tab.label}</Text>
            </View>
          </Link>
        );
      })}
    </View>
  );
}

function Header() {
  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' }}>
      <Link href="/" accessibilityLabel="Chaudron, accueil">
        <Logo size={24} />
      </Link>
      <Link href="/compte" accessibilityLabel="Ton compte">
        <Avatar />
      </Link>
    </View>
  );
}

/** Initiales du compte connecté, dans une pastille : le chemin vers le compte. */
export function Avatar({ size = 36 }: { size?: number }) {
  const { palette, fonts, weights } = useTheme();
  const { me } = useSession();
  const initials = (me?.display_name ?? '')
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase())
    .join('');
  return (
    <View style={{ width: size, height: size, borderRadius: size / 2, backgroundColor: palette.tagBg, alignItems: 'center', justifyContent: 'center' }}>
      {initials ? (
        <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: size * 0.4, color: palette.tagText }}>{initials}</Text>
      ) : (
        <Icon name="user" size={size * 0.55} color={palette.tagText} />
      )}
    </View>
  );
}

type NavEntry = { href: Href; label: string; icon: IconName; match: (path: string) => boolean };

const navigation: NavEntry[] = [
  { href: '/', label: 'Cette semaine', icon: 'week', match: (p) => p === '/' },
  { href: '/menus', label: 'Menus', icon: 'menus', match: (p) => p.startsWith('/menus') },
  { href: '/courses', label: 'Courses', icon: 'basket', match: (p) => p.startsWith('/courses') },
  { href: '/recettes', label: 'Recettes', icon: 'recipes', match: (p) => p.startsWith('/recettes') },
  { href: '/foyer', label: 'Le foyer', icon: 'home', match: (p) => p === '/foyer' || p.startsWith('/foyer/membres') || p === '/foyer/acces' },
  { href: '/foyer/equipements', label: 'Le matériel', icon: 'tools', match: (p) => p === '/foyer/equipements' },
];
/** Téléphone : le matériel est dans le foyer. */
const tabs: NavEntry[] = [
  { href: '/', label: 'Semaine', icon: 'week', match: (p) => p === '/' },
  { href: '/menus', label: 'Menus', icon: 'menus', match: (p) => p.startsWith('/menus') },
  { href: '/courses', label: 'Courses', icon: 'basket', match: (p) => p.startsWith('/courses') },
  { href: '/recettes', label: 'Recettes', icon: 'recipes', match: (p) => p.startsWith('/recettes') },
  { href: '/foyer', label: 'Foyer', icon: 'home', match: (p) => p.startsWith('/foyer') || p === '/compte' },
];
/** La suite de la V1, montrée pour donner la structure de l'appli. */
const upcoming: string[] = [];

/** Bureau : logo, navigation, puis le compte en bas. */
function Sidebar() {
  const { palette, fonts, weights, radius } = useTheme();
  const { me } = useSession();
  const path = usePathname();
  const entry = (active: boolean, icon: ReactNode, text: string) => (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: space[3], paddingVertical: space[3], paddingHorizontal: space[4], borderRadius: radius.button, backgroundColor: active ? palette.chip : 'transparent' }}>
      {icon}
      <Text style={{ fontFamily: fonts.text, fontWeight: active ? weights.strong : weights.medium, fontSize: 17, color: palette.text }}>{text}</Text>
    </View>
  );
  return (
    <View style={{ width: 248, paddingVertical: space[6], paddingHorizontal: space[3], gap: space[6], borderRightWidth: 1, borderRightColor: palette.line, backgroundColor: palette.card }}>
      <Link href="/" accessibilityLabel="Chaudron, accueil" style={{ paddingHorizontal: space[4] }}>
        <Logo size={28} />
      </Link>
      <View accessibilityRole="menu" style={{ gap: space[1] }}>
        {navigation.map((nav) => {
          const active = nav.match(path);
          return (
            <Link key={nav.label} href={nav.href} style={{ textDecorationLine: 'none' }}>
              {entry(active, <Icon name={nav.icon} size={22} color={active ? palette.link : palette.muted} />, nav.label)}
            </Link>
          );
        })}
        {upcoming.map((label) => (
          <Text key={label} style={{ paddingHorizontal: space[4], fontFamily: fonts.text, fontSize: 17, color: palette.muted, opacity: 0.7 }}>
            {label}
          </Text>
        ))}
      </View>
      <View style={{ marginTop: 'auto' }}>
        <Link href="/compte" style={{ textDecorationLine: 'none' }}>
          {entry(path === '/compte', <Avatar size={28} />, me?.display_name?.split(' ')[0] ?? 'Compte')}
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
    <Link href={href} style={{ paddingVertical: space[2], textDecorationLine: 'none' }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 2 }}>
        <Icon name="back" size={18} color={palette.link} strokeWidth={2.4} />
        <Text style={{ color: palette.link, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 16 }}>{label}</Text>
      </View>
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
  const size = level === 1 ? (wide ? 40 : textSize.h1) : wide ? textSize.h2 : 20;
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

/** Texte long replié à trois lignes, avec « Lire la suite » (présentation d'une recette, notes d'un menu). */
export function Clamped({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  const { palette, fonts, weights } = useTheme();
  const long = text.length > 180;
  return (
    <View style={{ gap: space[1], maxWidth: 720 }}>
      <Body tone="muted" style={long && !open ? ({ display: '-webkit-box', WebkitLineClamp: 3, WebkitBoxOrient: 'vertical', overflow: 'hidden' } as object) : undefined}>
        {text}
      </Body>
      {long ? (
        <Pressable onPress={() => setOpen(!open)} accessibilityRole="button" accessibilityState={{ expanded: open }} style={{ alignSelf: 'flex-start', paddingVertical: space[1] }}>
          <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: textSize.small, color: palette.link }}>{open ? 'Replier' : 'Lire la suite'}</Text>
        </Pressable>
      ) : null}
    </View>
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

type BadgeTone = 'neutral' | 'strong' | 'warn';

/**
 * Pastille d'information. `neutral` (par défaut) : calme, pour les étiquettes et les durées ; `strong` : ce qui
 * décide du soir (express) ; `warn` : à surveiller. Une icône facultative la précède.
 */
export function Tag({ children, tone = 'neutral', icon }: { children: ReactNode; tone?: BadgeTone; icon?: IconName }) {
  const { palette, fonts, radius, weights } = useTheme();
  const colors = {
    neutral: { bg: palette.chip, fg: palette.text },
    strong: { bg: palette.tagBg, fg: palette.tagText },
    warn: { bg: palette.chip, fg: palette.warn },
  }[tone];
  return (
    <View style={{ alignSelf: 'flex-start', flexDirection: 'row', alignItems: 'center', gap: 5, backgroundColor: colors.bg, borderRadius: radius.tag, paddingVertical: 5, paddingHorizontal: icon ? 10 : 12 }}>
      {icon ? <Icon name={icon} size={15} color={colors.fg} strokeWidth={2.2} /> : null}
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 14.5, color: colors.fg }}>{children}</Text>
    </View>
  );
}

/** Nom d'un plat en deux temps : le plat, en gras, puis sa garniture, plus discrète. `display` : police de titre. */
export function DishName({ title, size = 17, display = false, lines, color }: { title: string; size?: number; display?: boolean; lines?: number; color?: string }) {
  const { palette, fonts, weights } = useTheme();
  const { main, rest } = splitTitle(title);
  const restSize = display ? Math.max(16, Math.round(size * 0.6)) : Math.round(size * 0.85);
  return (
    <View style={{ gap: display ? space[1] : 1, minWidth: 0 }}>
      <Text
        numberOfLines={lines}
        style={{ fontFamily: display ? fonts.title : fonts.text, fontWeight: display ? weights.title : weights.strong, fontSize: size, lineHeight: Math.round(size * (display ? 1.15 : 1.3)), color: color ?? palette.text }}
      >
        {main}
      </Text>
      {rest ? (
        <Text numberOfLines={lines} style={{ fontFamily: fonts.text, fontSize: restSize, lineHeight: Math.round(restSize * 1.4), color: color ?? palette.muted, opacity: color ? 0.8 : 1 }}>
          {rest}
        </Text>
      ) : null}
    </View>
  );
}

/**
 * Vignette d'une recette, en attendant les photos : sa teinte de cuisine et le pictogramme de son appareil.
 * `size` : carré ; `height` : bandeau sur toute la largeur.
 */
export function Vignette({ hue, pictogram, size = 56, height }: { hue: Hue; pictogram: Pictogram; size?: number; height?: number }) {
  const { palette, mode, radius } = useTheme();
  const h = height ?? size;
  const icon = Math.round(Math.min(h * 0.52, 64));
  return (
    <View
      aria-hidden
      style={{ width: height ? '100%' : size, height: h, borderRadius: Math.min(radius.card, Math.round(h / 3.5)), backgroundColor: vignettes[mode][hue], alignItems: 'center', justifyContent: 'center' }}
    >
      <Icon name={pictogram} size={icon} color={palette.text} strokeWidth={icon > 40 ? 1.4 : 1.7} />
    </View>
  );
}

/** Barre de progression (courses prises). */
export function Progress({ value, label }: { value: number; label: string }) {
  const { palette } = useTheme();
  const pct = Math.max(0, Math.min(1, value));
  return (
    <View accessibilityRole="progressbar" aria-label={label} aria-valuenow={Math.round(pct * 100)} style={{ height: 10, borderRadius: 5, backgroundColor: palette.chip, overflow: 'hidden' }}>
      {pct > 0 ? (
        <View style={{ width: `${pct * 100}%`, height: '100%', borderRadius: 5, backgroundColor: palette.action, borderWidth: palette.actionBorder !== 'transparent' ? 1 : 0, borderColor: palette.actionBorder }} />
      ) : null}
    </View>
  );
}

/** Bouton rond à icône (« ⋯ », réorganiser) ; la cible tactile reste pleine. */
export function IconButton({ icon, label, onPress, active = false }: { icon: IconName; label: string; onPress: () => void; active?: boolean }) {
  const { palette } = useTheme();
  return (
    <Pressable
      onPress={onPress}
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ expanded: active }}
      style={({ pressed, hovered }: { pressed: boolean; hovered?: boolean }) => ({
        width: 44,
        height: 44,
        borderRadius: 22,
        alignItems: 'center',
        justifyContent: 'center',
        backgroundColor: active || hovered ? palette.chip : 'transparent',
        opacity: pressed ? 0.7 : 1,
      })}
    >
      <Icon name={icon} size={22} color={active ? palette.text : palette.muted} />
    </Pressable>
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
