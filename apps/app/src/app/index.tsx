import type { Agenda, MenuSlot, Warning } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { addDays, dayLabel, space, todayIso, weekDays } from '@chaudron/domain';
import { Redirect, Stack as Router } from 'expo-router';
import { type ComponentRef, type ReactNode, useRef, useState } from 'react';
import { View } from 'react-native';

import { api } from '../lib/api';
import { useSession } from '../lib/session';
import { useTheme } from '../lib/theme';
import { useLoad } from '../lib/useLoad';
import { Body, Button, Card, ErrorText, LinkButton, Loading, Row, Screen, Stack, Title, useWide } from '../ui';
import { DragHandle, mealLabels, modeHelp, type MoveRequest, MovePanel, SlotSummary, TextAction, Warnings, warningsOf } from '../ui/menu';

/** Accueil : connexion, création du foyer, ou les repas de la semaine. */
export default function Home() {
  const { me } = useSession();
  if (me === undefined) {
    return (
      <Screen header={false}>
        <Loading />
      </Screen>
    );
  }
  if (!me) return <Redirect href="/connexion" />;
  const home = me.households[0];
  if (!home) return <Redirect href="/foyer/nouveau" />;
  return <Week householdId={home.id} />;
}

const DAYS = 7;

type ViewRef = ComponentRef<typeof View>;

interface Drag {
  slot: MenuSlot;
  dy: number;
  over: string | null;
}

/** Les 7 jours qui viennent, tous menus confondus ; on glisse un plat d'un jour à l'autre. */
function Week({ householdId }: { householdId: string }) {
  const today = todayIso();
  // Un jour de plus : la phase de la veille du dernier plat se rappelle le septième jour.
  const agenda = useLoad(
    () => api.GET('/api/v1/households/{id}/agenda', { params: { path: { id: householdId }, query: { from: today, days: DAYS + 1 } } }),
    [householdId, today],
  );
  const [panel, setPanel] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [dropped, setDropped] = useState<{ slot: MenuSlot; date: string } | null>(null);
  const days = useRef(new Map<string, ViewRef>());
  const boxes = useRef(new Map<string, { y: number; h: number }>());

  const data = agenda.data;
  if (!data) {
    return (
      <Screen>
        <Router.Screen options={{ title: 'Cette semaine · Chaudron' }} />
        <Title>Cette semaine</Title>
        {agenda.error ? <ErrorText>{agenda.error}</ErrorText> : <Loading />}
      </Screen>
    );
  }

  const occupied = (date: string, meal: MenuSlot['meal'], of: MenuSlot) =>
    data.slots.filter((s) => s.date === date && s.meal === meal && s.week_start === of.week_start && s.id !== of.id);

  async function move(request: MoveRequest, weekStart: string) {
    setBusy(true);
    setError(null);
    try {
      const { error: failure } = await api.POST('/api/v1/households/{id}/menus/{week_start}/move', {
        params: { path: { id: householdId, week_start: weekStart } },
        body: request,
      });
      if (failure) setError(errorMessage(failure));
      else {
        setPanel(null);
        setDropped(null);
        await agenda.reload();
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  const measure = () => {
    for (const [date, view] of days.current) {
      view.measureInWindow((_x, y, _w, h) => boxes.current.set(date, { y, h }));
    }
  };
  const dayAt = (pageY: number) => {
    for (const [date, box] of boxes.current) if (pageY >= box.y && pageY <= box.y + box.h) return date;
    return null;
  };
  const drop = (slot: MenuSlot, pageY: number) => {
    const over = dayAt(pageY);
    setDrag(null);
    if (!over || over === slot.date) return;
    if (occupied(over, slot.meal, slot).length) setDropped({ slot, date: over });
    else void move({ slot: slot.id, date: over, meal: slot.meal, mode: 'insert' }, slot.week_start);
  };

  const shown = weekDays(today, DAYS);
  const empty = !data.slots.some((s) => s.date < addDays(today, DAYS));
  return (
    <Screen>
      <Router.Screen options={{ title: 'Cette semaine · Chaudron' }} />
      <Title>Cette semaine</Title>
      <ErrorText>{error}</ErrorText>
      {empty ? (
        <Card>
          <Body>Rien de prévu pour les sept jours qui viennent.</Body>
          <LinkButton kind="primary" label="Composer un menu" href="/menus" />
        </Card>
      ) : null}
      <Grid>
      {shown.map((date) => (
        <Day
          key={date}
          date={date}
          today={today}
          agenda={data}
          register={(view) => (view ? days.current.set(date, view) : days.current.delete(date))}
          drag={drag}
          renderSlot={(slot, big) => (
            <SlotBlock
              key={slot.id}
              slot={slot}
              big={big}
              warnings={warningsOf(data.warnings, slot.id).filter((w) => w.date === slot.date)}
              drag={drag?.slot.id === slot.id ? drag : null}
              panel={panel === slot.id}
              busy={busy}
              today={today}
              onPanel={() => setPanel(panel === slot.id ? null : slot.id)}
              onMove={(request) => void move(request, slot.week_start)}
              occupied={(d, meal) => occupied(d, meal, slot)}
              onDragStart={() => {
                measure();
                setDrag({ slot, dy: 0, over: slot.date });
              }}
              onDrag={(dy, pageY) => setDrag({ slot, dy, over: dayAt(pageY) })}
              onDrop={(pageY) => drop(slot, pageY)}
            />
          )}
        >
          {dropped && dropped.date === date ? (
            <DropChoice
              slot={dropped.slot}
              date={date}
              today={today}
              taken={occupied(date, dropped.slot.meal, dropped.slot)}
              busy={busy}
              onChoose={(mode) => void move({ slot: dropped.slot.id, date, meal: dropped.slot.meal, mode }, dropped.slot.week_start)}
              onCancel={() => setDropped(null)}
            />
          ) : null}
        </Day>
      ))}
      </Grid>
    </Screen>
  );
}

/** Les jours en grille sur bureau, empilés sur téléphone. */
function Grid({ children }: { children: ReactNode }) {
  const wide = useWide();
  if (!wide) return <View style={{ gap: space[5] }}>{children}</View>;
  return <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space[5], alignItems: 'flex-start' }}>{children}</View>;
}

/** Un jour : ses repas, ses rappels (ce qui se prépare la veille pour demain), ses avertissements. */
function Day({ date, today, agenda, register, drag, renderSlot, children }: { date: string; today: string; agenda: Agenda; register: (view: ViewRef | null) => void; drag: Drag | null; renderSlot: (slot: MenuSlot, big: boolean) => ReactNode; children?: ReactNode }) {
  const { palette, radius } = useTheme();
  const slots = agenda.slots.filter((s) => s.date === date);
  const lunch = slots.filter((s) => s.meal === 'lunch');
  const dinner = slots.filter((s) => s.meal === 'dinner');
  const tomorrow = agenda.slots.filter((s) => s.date === addDays(date, 1) && s.prep_days_before === 1);
  const own = new Set(slots.map((s) => s.id));
  // Avertissements du jour qui ne portent pas sur un plat du jour (le midi sans boîtes d'un plat déplacé).
  const loose: Warning[] = agenda.warnings.filter((w) => w.date === date && !w.slots.some((id) => own.has(id)));
  const isToday = date === today;
  const target = drag && drag.over === date && drag.slot.date !== date;
  const wide = useWide();
  // Le repas n'est nommé que s'il y en a deux, ou ce soir.
  const named = (lunch.length > 0 && dinner.length > 0) || isToday;
  return (
    <View
      ref={register}
      style={
        {
          width: wide ? 'calc(50% - 12px)' : '100%',
          gap: space[4],
          padding: space[5],
          borderRadius: radius.card,
          backgroundColor: palette.card,
          borderWidth: 2,
          borderStyle: target ? 'dashed' : 'solid',
          borderColor: target ? palette.link : isToday ? palette.actionBorder : 'transparent',
        } as never
      }
    >
      <Title level={2}>{dayLabel(date, today)}</Title>
      {lunch.length ? <Meal label={named ? 'Midi' : null}>{lunch.map((s) => renderSlot(s, false))}</Meal> : null}
      {dinner.length ? <Meal label={named ? (isToday ? 'Ce soir' : 'Soir') : null}>{dinner.map((s) => renderSlot(s, isToday))}</Meal> : null}
      {!slots.length ? <Body tone="muted" size={16}>Rien de prévu.</Body> : null}
      {tomorrow.map((s) => (
        <Body key={s.id} size={16}>{`Pour demain : ${s.recipe.title} se prépare en partie la veille.`}</Body>
      ))}
      <Warnings warnings={loose} />
      {children}
    </View>
  );
}

function Meal({ label, children }: { label: string | null; children: ReactNode }) {
  return (
    <View style={{ gap: space[4] }}>
      {label ? (
        <Body tone="muted" size={14} style={{ textTransform: 'uppercase', letterSpacing: 1 }}>
          {label}
        </Body>
      ) : null}
      {children}
    </View>
  );
}

function SlotBlock({ slot, big, warnings, drag, panel, busy, today, onPanel, onMove, occupied, onDragStart, onDrag, onDrop }: {
  slot: MenuSlot;
  big: boolean;
  warnings: Warning[];
  drag: Drag | null;
  panel: boolean;
  busy: boolean;
  today: string;
  onPanel: () => void;
  onMove: (request: MoveRequest) => void;
  occupied: (date: string, meal: MenuSlot['meal']) => MenuSlot[];
  onDragStart: () => void;
  onDrag: (dy: number, pageY: number) => void;
  onDrop: (pageY: number) => void;
}) {
  const { palette, radius } = useTheme();
  return (
    <View
      style={[
        { gap: space[3] },
        drag ? { transform: [{ translateY: drag.dy }], zIndex: 10, opacity: 0.92, backgroundColor: palette.card, borderRadius: radius.card, boxShadow: '0 8px 24px rgba(0,0,0,0.25)' } : null,
      ]}
    >
      <SlotSummary
        slot={slot}
        big={big}
        warnings={warnings}
        handle={<DragHandle label={`Glisser ${slot.recipe.title} vers un autre jour`} onStart={onDragStart} onDrag={onDrag} onDrop={onDrop} />}
        actions={panel ? null : <TextAction label="Déplacer…" onPress={onPanel} />}
      />
      {panel ? <MovePanel slot={slot} today={today} occupied={occupied} onMove={onMove} onCancel={onPanel} busy={busy} /> : null}
    </View>
  );
}

/** Le plat est lâché sur un repas déjà pris : intercaler, échanger ou servir ensemble. */
function DropChoice({ slot, date, today, taken, busy, onChoose, onCancel }: { slot: MenuSlot; date: string; today: string; taken: MenuSlot[]; busy: boolean; onChoose: (mode: 'insert' | 'swap' | 'together') => void; onCancel: () => void }) {
  return (
    <Card>
      <Body size={16}>{`${slot.recipe.title} sur ${dayLabel(date, today).toLowerCase()} ${mealLabels[slot.meal]}, déjà pris par ${taken.map((s) => s.recipe.title).join(', ')} :`}</Body>
      <Stack gap={space[2]}>
        <Button kind="primary" label="Intercaler" busy={busy} onPress={() => onChoose('insert')} />
        <Body tone="muted" size={14}>{modeHelp.insert}</Body>
        <Row>
          <Button label="Échanger" onPress={() => onChoose('swap')} />
          <Button label="Même soir" onPress={() => onChoose('together')} />
          <Button kind="quiet" label="Annuler" onPress={onCancel} />
        </Row>
      </Stack>
    </Card>
  );
}
