import { space, themeIds, themes } from '@chaudron/domain';
import { Pressable, Text, View } from 'react-native';

import { useWide } from '../ui';
import { useTheme } from './theme';

/** Choix du thème : chaque carte est un aperçu dans les couleurs et la police du thème, au mode courant. */
export function ThemePicker() {
  const { theme: current, mode, palette, radius, setTheme } = useTheme();
  const wide = useWide();
  return (
    <View
      accessibilityRole="radiogroup"
      aria-label="Thème"
      style={wide ? { flexDirection: 'row', flexWrap: 'wrap', gap: space[3] } : { gap: space[3] }}
    >
      {themeIds.map((id) => {
        const theme = themes[id];
        const p = theme.palettes[mode];
        const selected = id === current.id;
        return (
          <Pressable
            key={id}
            onPress={() => setTheme(id)}
            accessibilityRole="radio"
            accessibilityState={{ checked: selected }}
            accessibilityLabel={`Thème ${theme.name}`}
            style={({ pressed }) => ({
              backgroundColor: p.bg,
              borderRadius: radius.card,
              borderWidth: 2,
              borderColor: selected ? palette.text : palette.line,
              padding: space[4],
              gap: space[2],
              ...(wide ? { flexBasis: 280, flexGrow: 1 } : null),
              opacity: pressed ? 0.85 : 1,
            })}
          >
            <View style={{ flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: space[3] }}>
              <Text style={{ fontFamily: theme.fonts.title, fontWeight: theme.weights.title, fontSize: 20, color: p.text }}>
                {theme.name}
              </Text>
              <View style={{ flexDirection: 'row', gap: 6 }} aria-hidden>
                {[p.action, p.tagBg, p.card, p.logoO].map((color, i) => (
                  <View key={i} style={{ width: 18, height: 18, borderRadius: 9, backgroundColor: color, borderWidth: 1, borderColor: p.line }} />
                ))}
              </View>
            </View>
            <Text style={{ fontFamily: theme.fonts.text, fontSize: 15, lineHeight: 22, color: p.muted }}>{theme.description}</Text>
            <View style={{ flexDirection: 'row', gap: space[2], alignItems: 'center' }}>
              <View style={{ backgroundColor: p.action, borderColor: p.actionBorder, borderWidth: 2, borderRadius: theme.radius.button, paddingHorizontal: 14, paddingVertical: 6 }}>
                <Text style={{ fontFamily: theme.fonts.text, fontWeight: theme.weights.medium, fontSize: 14, color: p.onAction }}>Valider</Text>
              </View>
              <View style={{ backgroundColor: p.tagBg, borderRadius: theme.radius.tag, paddingHorizontal: 12, paddingVertical: 6 }}>
                <Text style={{ fontFamily: theme.fonts.text, fontWeight: theme.weights.medium, fontSize: 14, color: p.tagText }}>Cookeo</Text>
              </View>
              {selected ? (
                <Text style={{ marginLeft: 'auto', fontFamily: theme.fonts.text, fontWeight: theme.weights.medium, fontSize: 14, color: p.text }}>
                  ✓ Choisi
                </Text>
              ) : null}
            </View>
          </Pressable>
        );
      })}
    </View>
  );
}
