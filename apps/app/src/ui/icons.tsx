// Icônes de l'appli, dessinées ici (pas de bibliothèque) : traits de 2 sur une grille de 24, bouts arrondis.
// Pictogrammes des vignettes : les appareils de cuisine, au même trait.
import type { Pictogram } from '@chaudron/domain';
import type { ReactNode } from 'react';
import Svg, { Circle, Line, Path, Rect } from 'react-native-svg';

export type IconName =
  | 'week'
  | 'menus'
  | 'basket'
  | 'recipes'
  | 'home'
  | 'tools'
  | 'user'
  | 'bolt'
  | 'moon'
  | 'alert'
  | 'dislike'
  | 'snow'
  | 'info'
  | 'more'
  | 'grip'
  | 'clock'
  | 'check'
  | 'chevron'
  | 'back'
  | 'search'
  | 'plus'
  | 'people'
  | 'note'
  | 'bell'
  | Pictogram;

const shapes: Record<IconName, ReactNode> = {
  week: (
    <>
      <Rect x={3.5} y={5} width={17} height={15.5} rx={3} />
      <Line x1={3.5} y1={10} x2={20.5} y2={10} />
      <Line x1={8} y1={3} x2={8} y2={6.5} />
      <Line x1={16} y1={3} x2={16} y2={6.5} />
      <Circle cx={12} cy={15} r={1.2} fill="currentColor" />
    </>
  ),
  menus: (
    <>
      <Rect x={5} y={3.5} width={14} height={17} rx={2.5} />
      <Line x1={8.5} y1={8.5} x2={15.5} y2={8.5} />
      <Line x1={8.5} y1={12} x2={15.5} y2={12} />
      <Line x1={8.5} y1={15.5} x2={12.5} y2={15.5} />
    </>
  ),
  basket: (
    <>
      <Path d="M3.5 9.5h17l-1.8 9a2 2 0 0 1-2 1.5H7.3a2 2 0 0 1-2-1.5z" />
      <Path d="M8 9.5 11 4M16 9.5 13 4" />
      <Line x1={9.5} y1={13} x2={9.5} y2={16.5} />
      <Line x1={14.5} y1={13} x2={14.5} y2={16.5} />
    </>
  ),
  recipes: (
    <>
      <Path d="M5 4.5h10.5a3 3 0 0 1 3 3v12H8a3 3 0 0 1-3-3z" />
      <Path d="M5 16.5a3 3 0 0 1 3-3h10.5" />
      <Line x1={9} y1={8.5} x2={14.5} y2={8.5} />
    </>
  ),
  home: (
    <>
      <Path d="M4 10.5 12 4l8 6.5" />
      <Path d="M6 9v10.5h12V9" />
      <Path d="M10 19.5v-5h4v5" />
    </>
  ),
  tools: (
    <>
      <Path d="M5 9.5h14v6a4 4 0 0 1-4 4H9a4 4 0 0 1-4-4z" />
      <Path d="M3 11.5h2M19 11.5h2" />
      <Path d="M9 6.5c0-1.2 1-1.2 1-2.4M13 6.5c0-1.2 1-1.2 1-2.4" />
    </>
  ),
  user: (
    <>
      <Circle cx={12} cy={8.5} r={4} />
      <Path d="M4.5 20c1-4 4-6 7.5-6s6.5 2 7.5 6" />
    </>
  ),
  bolt: <Path d="M13 3 5.5 13.5H12L11 21l7.5-10.5H12z" />,
  moon: <Path d="M19.5 14.5A8 8 0 0 1 9.5 4.5a8 8 0 1 0 10 10z" />,
  alert: (
    <>
      <Path d="M10.3 4.3a2 2 0 0 1 3.4 0l7 12.2a2 2 0 0 1-1.7 3H5a2 2 0 0 1-1.7-3z" />
      <Line x1={12} y1={9.5} x2={12} y2={13.5} />
      <Circle cx={12} cy={16.5} r={0.6} fill="currentColor" />
    </>
  ),
  dislike: (
    <>
      <Circle cx={12} cy={12} r={8.5} />
      <Line x1={6} y1={18} x2={18} y2={6} />
    </>
  ),
  snow: (
    <>
      <Line x1={12} y1={3} x2={12} y2={21} />
      <Line x1={4.2} y1={7.5} x2={19.8} y2={16.5} />
      <Line x1={4.2} y1={16.5} x2={19.8} y2={7.5} />
      <Path d="M9.5 4.5 12 6.5l2.5-2M9.5 19.5 12 17.5l2.5 2" />
    </>
  ),
  info: (
    <>
      <Circle cx={12} cy={12} r={8.5} />
      <Line x1={12} y1={11} x2={12} y2={16} />
      <Circle cx={12} cy={8} r={0.6} fill="currentColor" />
    </>
  ),
  more: (
    <>
      <Circle cx={6} cy={12} r={1.3} fill="currentColor" />
      <Circle cx={12} cy={12} r={1.3} fill="currentColor" />
      <Circle cx={18} cy={12} r={1.3} fill="currentColor" />
    </>
  ),
  grip: (
    <>
      <Circle cx={9} cy={7} r={1.2} fill="currentColor" />
      <Circle cx={15} cy={7} r={1.2} fill="currentColor" />
      <Circle cx={9} cy={12} r={1.2} fill="currentColor" />
      <Circle cx={15} cy={12} r={1.2} fill="currentColor" />
      <Circle cx={9} cy={17} r={1.2} fill="currentColor" />
      <Circle cx={15} cy={17} r={1.2} fill="currentColor" />
    </>
  ),
  clock: (
    <>
      <Circle cx={12} cy={12} r={8.5} />
      <Path d="M12 7.5V12l3 2" />
    </>
  ),
  check: <Path d="M5 12.5 10 17.5 19 7" />,
  people: (
    <>
      <Circle cx={9} cy={8.5} r={3.5} />
      <Path d="M2.5 19.5c.8-3.5 3.4-5.5 6.5-5.5s5.7 2 6.5 5.5" />
      <Path d="M15.5 5.2a3.5 3.5 0 0 1 0 6.6M17.5 14.4c2 .7 3.4 2.4 4 5.1" />
    </>
  ),
  note: (
    <>
      <Path d="M5 4.5h14v10l-5 5H5z" />
      <Path d="M14 19.5v-5h5" />
      <Line x1={8.5} y1={9} x2={15.5} y2={9} />
      <Line x1={8.5} y1={12.5} x2={12} y2={12.5} />
    </>
  ),
  bell: (
    <>
      <Path d="M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15z" />
      <Path d="M10 20.5a2 2 0 0 0 4 0" />
    </>
  ),
  chevron: <Path d="M9.5 5.5 16 12l-6.5 6.5" />,
  back: <Path d="M14.5 5.5 8 12l6.5 6.5" />,
  search: (
    <>
      <Circle cx={10.5} cy={10.5} r={6} />
      <Line x1={15} y1={15} x2={20} y2={20} />
    </>
  ),
  plus: (
    <>
      <Line x1={12} y1={5} x2={12} y2={19} />
      <Line x1={5} y1={12} x2={19} y2={12} />
    </>
  ),
  // Pictogrammes des vignettes
  pot: (
    <>
      <Path d="M4.5 10.5h15v5.5a4 4 0 0 1-4 4h-7a4 4 0 0 1-4-4z" />
      <Path d="M3.5 10.5h17" />
      <Path d="M10 8h4" />
      <Path d="M2.5 13h2M19.5 13h2" />
      <Path d="M9 5.5c0-1 .8-1 .8-2M14 5.5c0-1 .8-1 .8-2" />
    </>
  ),
  oven: (
    <>
      <Rect x={3.5} y={3.5} width={17} height={17} rx={2.5} />
      <Line x1={3.5} y1={8} x2={20.5} y2={8} />
      <Rect x={7} y={11} width={10} height={6.5} rx={1.5} />
      <Circle cx={7} cy={5.8} r={0.6} fill="currentColor" />
      <Circle cx={10} cy={5.8} r={0.6} fill="currentColor" />
    </>
  ),
  pan: (
    <>
      <Circle cx={10} cy={13} r={6.5} />
      <Path d="M15.5 9.5 21 5" />
      <Path d="M8 12.5c1-1.5 3-1.5 4 0" />
    </>
  ),
  blender: (
    <>
      <Path d="M7 3.5h10l-1.5 11h-7z" />
      <Path d="M7.5 14.5h9l1 6h-11z" />
      <Line x1={17} y1={7} x2={19.5} y2={7} />
      <Circle cx={12} cy={17.5} r={0.6} fill="currentColor" />
    </>
  ),
  microwave: (
    <>
      <Rect x={2.5} y={5} width={19} height={14} rx={2.5} />
      <Rect x={5.5} y={8} width={9.5} height={8} rx={1.2} />
      <Line x1={18.2} y1={8.5} x2={18.2} y2={9.5} />
      <Line x1={18.2} y1={12.5} x2={18.2} y2={15.5} />
    </>
  ),
  bowl: (
    <>
      <Path d="M3 11.5h18a9 9 0 0 1-18 0z" />
      <Path d="M8 20.5h8" />
      <Path d="M13.5 8.5c0-2.5 2-4.5 4.5-4.5 0 2.5-2 4.5-4.5 4.5zM13.5 8.5 11 6" />
    </>
  ),
};

/** Icône au trait, dans la couleur donnée. `strokeWidth` s'ajuste aux grandes tailles (vignettes). */
export function Icon({ name, size = 22, color, strokeWidth = 2 }: { name: IconName; size?: number; color: string; strokeWidth?: number }) {
  return (
    <Svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" color={color} aria-hidden>
      {shapes[name]}
    </Svg>
  );
}
