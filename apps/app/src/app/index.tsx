import type { Agenda, MenuSlot, Warning } from '@chaudron/api';
import { errorMessage } from '@chaudron/api';
import { addDays, dayLabel, shortDateText, space, splitTitle, todayIso, weekDays, weekdaysLetter } from '@chaudron/domain';
import { Link, Redirect, Stack as Router } from 'expo-router';
import { type ComponentRef, type ReactNode, useEffect, useRef, useState } from 'react';
import { Platform, Pressable, Text, View } from 'react-native';

import { api } from '../lib/api';
import { useSession } from '../lib/session';
import { useTheme } from '../lib/theme';
import { setItem } from '../lib/store';
import { useCached } from '../lib/useCached';
import { Body, Button, Card, DishName, ErrorText, IconButton, Loading, Row, Screen, Stack, Title, useWide } from '../ui';
import { Icon, type IconName } from '../ui/icons';
import { DragHandle, mealLabels, modeHelp, type MoveRequest, MovePanel, NoticeLine, SlotBadges, slotHref, slotMeta, SlotSummary, SlotVignette, Warnings, warningsOf } from '../ui/menu';

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

/** Ce que l'écran sait faire d'un plat : le déplacer, congeler ce qui ne tiendra pas. */
interface SlotActions {
  panel: string | null;
  busy: boolean;
  today: string;
  setPanel: (id: string | null) => void;
  move: (request: MoveRequest, weekStart: string) => void;
  freeze: (slot: MenuSlot, ingredient: string) => void;
  occupied: (date: string, meal: MenuSlot['meal'], of: MenuSlot) => MenuSlot[];
}

/**
 * Les 7 jours qui viennent, tous menus confondus. Le soir du jour en grand ; la suite en liste, les jours vides
 * regroupés. « Réorganiser » montre chaque jour et les poignées pour glisser un plat d'un jour à l'autre.
 */
function Week({ householdId }: { householdId: string }) {
  const today = todayIso();
  const wide = useWide();
  // Deux semaines : la copie de travail de l'appareil (cette semaine et la suivante) ; on en montre sept jours.
  const agenda = useCached(`agenda:${householdId}`, () =>
    api.GET('/api/v1/households/{id}/agenda', { params: { path: { id: householdId }, query: { from: today, days: 2 * DAYS } } }),
  );
  usePrefetch(householdId, agenda.offline ? undefined : agenda.data);
  const [panel, setPanel] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [arranging, setArranging] = useState(false);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [dropped, setDropped] = useState<{ slot: MenuSlot; date: string } | null>(null);
  const days = useRef(new Map<string, ViewRef>());
  const boxes = useRef(new Map<string, { y: number; h: number }>());
  // Pas une ref : les rappels d'ancrage se créent au rendu.
  const [anchors] = useState(() => new Map<string, ViewRef>());

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

  async function freeze(slot: MenuSlot, ingredient: string) {
    setBusy(true);
    setError(null);
    try {
      const { error: failure } = await api.POST('/api/v1/households/{id}/menus/{week_start}/freeze', {
        params: { path: { id: householdId, week_start: slot.week_start } },
        body: { slot: slot.id, ingredient, frozen: true },
      });
      if (failure) setError(errorMessage(failure));
      else await agenda.reload();
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
  /** Web : le bandeau des jours fait défiler jusqu'au jour touché. */
  const scrollTo = (date: string) => {
    const view = anchors.get(date) as unknown as { scrollIntoView?: (options: object) => void } | undefined;
    view?.scrollIntoView?.({ behavior: 'smooth', block: 'start' });
  };
  const anchor = (dates: string[]) => (view: ViewRef | null) => {
    for (const d of dates) {
      if (view) anchors.set(d, view);
      else anchors.delete(d);
    }
  };

  const actions: SlotActions = { panel, busy, today, setPanel, move: (r, w) => void move(r, w), freeze: (s, i) => void freeze(s, i), occupied };
  const shown = weekDays(today, DAYS);
  const planned = new Set(data.slots.map((s) => s.date));
  const empty = !shown.some((d) => planned.has(d));
  const rest = shown.slice(1);

  /** Un plat, avec ses actions ; en réorganisation, la poignée. */
  const renderSlot = (slot: MenuSlot) => (
    <SlotBlock
      key={slot.id}
      slot={slot}
      warnings={warningsOf(data.warnings, slot.id).filter((w) => w.date === slot.date)}
      actions={actions}
      drag={drag?.slot.id === slot.id ? drag : null}
      handle={
        arranging ? (
          <DragHandle
            label={`Glisser ${slot.recipe.title} vers un autre jour`}
            onStart={() => {
              measure();
              setDrag({ slot, dy: 0, over: slot.date });
            }}
            onDrag={(dy, pageY) => setDrag({ slot, dy, over: dayAt(pageY) })}
            onDrop={(pageY) => drop(slot, pageY)}
          />
        ) : null
      }
    />
  );
  const dropChoice = (date: string) =>
    dropped && dropped.date === date ? (
      <DropChoice
        slot={dropped.slot}
        date={date}
        today={today}
        taken={occupied(date, dropped.slot.meal, dropped.slot)}
        busy={busy}
        onChoose={(mode) => void move({ slot: dropped.slot.id, date, meal: dropped.slot.meal, mode }, dropped.slot.week_start)}
        onCancel={() => setDropped(null)}
      />
    ) : null;

  return (
    <Screen>
      <Router.Screen options={{ title: 'Cette semaine · Chaudron' }} />
      <Row style={{ justifyContent: 'space-between', flexWrap: 'nowrap' }}>
        <Title>Cette semaine</Title>
        {empty ? null : arranging ? (
          <Button kind="quiet" label="Terminé" onPress={() => setArranging(false)} />
        ) : (
          <IconButton icon="grip" label="Réorganiser la semaine" onPress={() => setArranging(true)} />
        )}
      </Row>
      <WeekStrip days={shown} today={today} planned={planned} onPick={scrollTo} />
      {agenda.offline ? <Body tone="muted" size={15}>Hors ligne : la semaine gardée sur cet appareil.</Body> : null}
      <InstallHint />
      <ErrorText>{error}</ErrorText>

      {arranging ? (
        <>
          <Body tone="muted" size={15}>Tiens la poignée d’un plat et glisse-le sur un autre jour.</Body>
          <Card style={{ gap: 0, paddingVertical: space[2] }}>
            {shown.map((date, i) => (
              <DayRow
                key={date}
                date={date}
                today={today}
                agenda={data}
                last={i === shown.length - 1}
                register={(view) => {
                  if (view) days.current.set(date, view);
                  else days.current.delete(date);
                  anchor([date])(view);
                }}
                target={!!drag && drag.over === date && drag.slot.date !== date}
                renderSlot={renderSlot}
              >
                {dropChoice(date)}
              </DayRow>
            ))}
          </Card>
        </>
      ) : (
        <>
          <Tonight date={today} agenda={data} actions={actions} register={anchor([today])} />
          {empty ? null : wide ? (
            <Planner days={rest} today={today} agenda={data} register={(d) => anchor([d])} />
          ) : (
            <Card style={{ gap: 0, paddingVertical: space[2] }}>
              {groupEmpty(rest, planned).map((group, i, all) =>
                group.length > 1 || !planned.has(group[0]) ? (
                  <EmptyRange key={group[0]} dates={group} today={today} last={i === all.length - 1} register={anchor(group)} />
                ) : (
                  <DayRow key={group[0]} date={group[0]} today={today} agenda={data} last={i === all.length - 1} register={anchor(group)} renderSlot={renderSlot}>
                    {dropChoice(group[0])}
                  </DayRow>
                ),
              )}
            </Card>
          )}
        </>
      )}
    </Screen>
  );
}

/** Jours qui se suivent sans rien de prévu, regroupés ; un jour prévu reste seul. */
function groupEmpty(dates: string[], planned: Set<string>): string[][] {
  const groups: string[][] = [];
  for (const d of dates) {
    const last = groups.at(-1);
    if (!planned.has(d) && last && !planned.has(last[0])) last.push(d);
    else groups.push([d]);
  }
  return groups;
}

const capitalize = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** Le bandeau des sept jours : initiale et date, un point sous les jours prévus, aujourd'hui sur l'accent. */
function WeekStrip({ days, today, planned, onPick }: { days: string[]; today: string; planned: Set<string>; onPick: (date: string) => void }) {
  const { palette, fonts, weights, radius } = useTheme();
  return (
    <View style={{ flexDirection: 'row', gap: space[1] }}>
      {days.map((date) => {
        const isToday = date === today;
        const has = planned.has(date);
        const weekday = (new Date(`${date}T00:00:00Z`).getUTCDay() + 6) % 7;
        return (
          <Pressable
            key={date}
            onPress={() => onPick(date)}
            accessibilityRole="button"
            accessibilityLabel={`${dayLabel(date, today)}${has ? '' : ', rien de prévu'}`}
            style={({ pressed }) => ({
              flex: 1,
              alignItems: 'center',
              gap: 2,
              paddingVertical: space[2],
              borderRadius: radius.button,
              backgroundColor: isToday ? palette.action : palette.card,
              borderWidth: isToday && palette.actionBorder !== 'transparent' ? 1.5 : 0,
              borderColor: palette.actionBorder,
              opacity: pressed ? 0.8 : 1,
            })}
          >
            <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 13, color: isToday ? palette.onAction : palette.muted }}>{weekdaysLetter[weekday]}</Text>
            <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 17, color: isToday ? palette.onAction : palette.text, fontVariant: ['tabular-nums'] }}>{Number(date.slice(8))}</Text>
            <View style={{ width: 6, height: 6, borderRadius: 3, backgroundColor: has ? (isToday ? palette.onAction : palette.link) : 'transparent' }} />
          </Pressable>
        );
      })}
    </View>
  );
}

/**
 * Le repas du jour, en grand, sur l'accent : le soir (ou le midi s'il n'y a que lui), la vignette, le nom, et ce
 * qu'il faut savoir dans un encart clair.
 */
function Tonight({ date, agenda, actions, register }: { date: string; agenda: Agenda; actions: SlotActions; register: (view: ViewRef | null) => void }) {
  const { palette, radius, fonts, weights } = useTheme();
  const wide = useWide();
  const slots = agenda.slots.filter((s) => s.date === date);
  const dinner = slots.filter((s) => s.meal === 'dinner');
  const lunch = slots.filter((s) => s.meal === 'lunch');
  const main = dinner.length ? dinner : lunch;
  const label = dinner.length ? 'Ce soir' : lunch.length ? 'Ce midi' : 'Aujourd’hui';
  const others = dinner.length ? lunch : [];
  const own = new Set(slots.map((s) => s.id));
  const loose = agenda.warnings.filter((w) => w.date === date && !w.slots.some((id) => own.has(id)));
  return (
    <View ref={register} style={{ gap: space[4], padding: space[5], borderRadius: radius.card + 4, backgroundColor: palette.action, borderWidth: palette.actionBorder !== 'transparent' ? 2 : 0, borderColor: palette.actionBorder }}>
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 16, color: palette.onAction }}>{label}</Text>
      {main.length === 0 ? (
        <Stack gap={space[3]}>
          <Text style={{ fontFamily: fonts.title, fontWeight: weights.title, fontSize: 24, lineHeight: 30, color: palette.onAction }}>Rien de prévu</Text>
          <Row>
            <AccentLink href="/menus" label="Composer un menu" />
          </Row>
        </Stack>
      ) : null}
      {main.map((slot) => {
        const warnings = warningsOf(agenda.warnings, slot.id).filter((w) => w.date === slot.date);
        const fresh = warnings.filter((w) => w.kind === 'freshness');
        const open = actions.panel === slot.id;
        return (
          <View key={slot.id} style={{ gap: space[4] }}>
            <View style={{ flexDirection: wide ? 'row' : 'column', gap: space[4], alignItems: wide ? 'center' : 'flex-start' }}>
              <Link href={slotHref(slot)} accessibilityLabel={slot.recipe.title}>
                <SlotVignette slot={slot} size={wide ? 120 : 84} />
              </Link>
              <View style={{ flex: wide ? 1 : undefined, gap: space[3], alignSelf: 'stretch' }}>
                <Link href={slotHref(slot)} style={{ textDecorationLine: 'none' }}>
                  <DishName title={slot.recipe.title} display size={wide ? 34 : 27} color={palette.onAction} />
                </Link>
                <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 16, color: palette.onAction }}>{slotMeta(slot)}</Text>
                <SlotBadges slot={slot} />
              </View>
            </View>
            {warnings.length - fresh.length > 0 || fresh.length || open ? (
              <View style={{ gap: space[3], padding: space[4], borderRadius: radius.card, backgroundColor: palette.card }}>
                <Warnings warnings={warnings.filter((w) => w.kind !== 'freshness')} />
                <Freshness slot={slot} warnings={fresh} today={actions.today} busy={actions.busy} onFreeze={(i) => actions.freeze(slot, i)} onMove={(r) => actions.move(r, slot.week_start)} />
                {open ? (
                  <MovePanel slot={slot} today={actions.today} occupied={(d, meal) => actions.occupied(d, meal, slot)} onMove={(r) => actions.move(r, slot.week_start)} onCancel={() => actions.setPanel(null)} busy={actions.busy} />
                ) : null}
              </View>
            ) : null}
            <Row gap={space[2]}>
              <AccentLink href={slotHref(slot)} label="Voir la recette" />
              {open ? null : <AccentButton label="Déplacer…" onPress={() => actions.setPanel(slot.id)} />}
            </Row>
          </View>
        );
      })}
      {others.map((slot) => (
        <Link key={slot.id} href={slotHref(slot)} style={{ textDecorationLine: 'none' }}>
          <Text style={{ fontFamily: fonts.text, fontSize: 15, color: palette.onAction }}>{`${mealLabels[slot.meal][0].toUpperCase()}${mealLabels[slot.meal].slice(1)} : ${splitTitle(slot.recipe.title).main}`}</Text>
        </Link>
      ))}
      <Reminders date={date} agenda={agenda} onAccent />
      {loose.length ? (
        <View style={{ padding: space[4], borderRadius: radius.card, backgroundColor: palette.card }}>
          <Warnings warnings={loose} />
        </View>
      ) : null}
    </View>
  );
}

/** Bouton plein sur le bandeau d'accent : la couleur du texte en fond. */
function AccentLink({ href, label }: { href: string; label: string }) {
  const { palette, fonts, weights, radius } = useTheme();
  return (
    <Link
      href={href as never}
      style={{ minHeight: 48, lineHeight: 48, paddingHorizontal: space[5], borderRadius: radius.button, backgroundColor: palette.text, color: palette.card, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 16, textDecorationLine: 'none', textAlign: 'center' }}
    >
      {label}
    </Link>
  );
}

function AccentButton({ label, onPress }: { label: string; onPress: () => void }) {
  const { palette, fonts, weights, radius } = useTheme();
  return (
    <Pressable
      onPress={onPress}
      accessibilityRole="button"
      style={({ pressed }) => ({ minHeight: 48, justifyContent: 'center', paddingHorizontal: space[4], borderRadius: radius.button, borderWidth: 1.5, borderColor: palette.onAction, opacity: pressed ? 0.7 : 1 })}
    >
      <Text style={{ fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 16, color: palette.onAction }}>{label}</Text>
    </Pressable>
  );
}

/** Rappels du jour pour le lendemain : ce qui se prépare la veille, ce qu'il faut sortir du congélateur. */
function Reminders({ date, agenda, onAccent = false }: { date: string; agenda: Agenda; onAccent?: boolean }) {
  const tomorrow = agenda.slots.filter((s) => s.date === addDays(date, 1) && s.prep_days_before === 1);
  const thaw = agenda.slots.filter((s) => s.date === addDays(date, 1) && s.thaw.length);
  if (!tomorrow.length && !thaw.length) return null;
  return (
    <Stack gap={space[2]}>
      {tomorrow.map((s) => (
        <Reminder key={s.id} icon="moon" onAccent={onAccent} text={`Pour demain : ${splitTitle(s.recipe.title).main} se prépare en partie ce soir.`} />
      ))}
      {thaw.map((s) => (
        <Reminder key={`thaw-${s.id}`} icon="snow" onAccent={onAccent} text={`Sortir du congélateur pour demain : ${s.thaw.map((t) => t.name.toLowerCase()).join(', ')}.`} />
      ))}
    </Stack>
  );
}

function Reminder({ icon, text, onAccent }: { icon: IconName; text: string; onAccent: boolean }) {
  const { palette, fonts, weights } = useTheme();
  const color = onAccent ? palette.onAction : palette.text;
  return (
    <View style={{ flexDirection: 'row', gap: space[2], alignItems: 'flex-start' }}>
      <View style={{ paddingTop: 2 }}>
        <Icon name={icon} size={17} color={color} strokeWidth={2.2} />
      </View>
      <Text style={{ flex: 1, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 15, lineHeight: 21, color }}>{text}</Text>
    </View>
  );
}

/** Le nom d'un jour, en tête de ligne. */
function DayName({ date, today, short = false }: { date: string; today: string; short?: boolean }) {
  const { palette, fonts, weights } = useTheme();
  const text = short && date !== addDays(today, 1) ? capitalize(shortDateText(date)) : dayLabel(date, today);
  return <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 15, color: palette.muted }}>{text}</Text>;
}

/** Une ligne de la liste : le jour, ses plats, ses rappels et avertissements. */
function DayRow({ date, today, agenda, last, register, target = false, renderSlot, children }: { date: string; today: string; agenda: Agenda; last: boolean; register: (view: ViewRef | null) => void; target?: boolean; renderSlot: (slot: MenuSlot) => ReactNode; children?: ReactNode }) {
  const { palette, radius } = useTheme();
  const slots = agenda.slots.filter((s) => s.date === date);
  const lunch = slots.filter((s) => s.meal === 'lunch');
  const dinner = slots.filter((s) => s.meal === 'dinner');
  const own = new Set(slots.map((s) => s.id));
  // Avertissements du jour qui ne portent pas sur un plat du jour (le midi sans boîtes d'un plat déplacé).
  const loose: Warning[] = agenda.warnings.filter((w) => w.date === date && !w.slots.some((id) => own.has(id)));
  // Le repas n'est nommé que s'il y en a deux.
  const named = lunch.length > 0 && dinner.length > 0;
  return (
    <View
      ref={register}
      style={{
        gap: space[3],
        paddingVertical: space[4],
        paddingHorizontal: target ? space[2] : 0,
        borderBottomWidth: last ? 0 : 1,
        borderBottomColor: palette.line,
        borderRadius: target ? radius.button : 0,
        backgroundColor: target ? palette.chip : 'transparent',
      }}
    >
      <DayName date={date} today={today} />
      {lunch.length ? <Meal label={named ? 'Midi' : null}>{lunch.map(renderSlot)}</Meal> : null}
      {dinner.length ? <Meal label={named ? 'Soir' : null}>{dinner.map(renderSlot)}</Meal> : null}
      {!slots.length ? <Body tone="muted" size={15}>Rien de prévu.</Body> : null}
      <Reminders date={date} agenda={agenda} />
      <Warnings warnings={loose} />
      {children}
    </View>
  );
}

/** Des jours sans rien de prévu, en une ligne : « Sam. 24 → mar. 27 ». */
function EmptyRange({ dates, today, last, register }: { dates: string[]; today: string; last: boolean; register: (view: ViewRef | null) => void }) {
  const { palette, fonts, weights } = useTheme();
  const span = dates.length > 1 ? `${capitalize(shortDateText(dates[0]))} → ${shortDateText(dates[dates.length - 1])}` : dayLabel(dates[0], today);
  return (
    <View ref={register} style={{ flexDirection: 'row', alignItems: 'center', gap: space[3], paddingVertical: space[4], borderBottomWidth: last ? 0 : 1, borderBottomColor: palette.line }}>
      <View style={{ flex: 1, gap: 2 }}>
        <Text style={{ fontFamily: fonts.text, fontWeight: weights.strong, fontSize: 15, color: palette.muted }}>{span}</Text>
        <Body tone="muted" size={15}>Rien de prévu.</Body>
      </View>
      <Link href="/menus" style={{ color: palette.link, fontFamily: fonts.text, fontWeight: weights.medium, fontSize: 15, padding: space[2] }}>
        Composer
      </Link>
    </View>
  );
}

function Meal({ label, children }: { label: string | null; children: ReactNode }) {
  return (
    <View style={{ gap: space[3] }}>
      {label ? <Body tone="muted" size={14}>{label}</Body> : null}
      {children}
    </View>
  );
}

/** Bureau : la suite de la semaine en colonnes, une par jour, comme un planning. */
function Planner({ days, today, agenda, register }: { days: string[]; today: string; agenda: Agenda; register: (date: string) => (view: ViewRef | null) => void }) {
  const { palette, radius, fonts } = useTheme();
  return (
    <View style={{ flexDirection: 'row', gap: space[3], alignItems: 'stretch' }}>
      {days.map((date) => {
        const slots = agenda.slots.filter((s) => s.date === date);
        return (
          <View key={date} ref={register(date)} style={{ flex: 1, minWidth: 0, gap: space[3], padding: space[4], borderRadius: radius.card, backgroundColor: slots.length ? palette.card : 'transparent', borderWidth: slots.length ? 0 : 1.5, borderColor: palette.line, borderStyle: 'dashed' }}>
            <DayName date={date} today={today} short />
            {slots.map((slot) => {
              const warnings = warningsOf(agenda.warnings, slot.id).filter((w) => w.date === slot.date);
              return (
                <Link key={slot.id} href={slotHref(slot)} style={{ textDecorationLine: 'none' }}>
                  <View style={{ gap: space[2] }}>
                    <SlotVignette slot={slot} size={48} />
                    <DishName title={slot.recipe.title} size={16} />
                    <Text style={{ fontFamily: fonts.text, fontSize: 14, color: palette.muted }}>{slotMeta(slot)}</Text>
                    <SlotBadges slot={slot} />
                    <Warnings warnings={warnings} />
                  </View>
                </Link>
              );
            })}
            {!slots.length ? <Body tone="muted" size={15}>Rien de prévu.</Body> : null}
            <Reminders date={date} agenda={agenda} />
          </View>
        );
      })}
    </View>
  );
}

/**
 * Garde sur l'appareil ce qu'il faut pour la semaine sans réseau : chaque recette de l'agenda (la fiche s'ouvre
 * hors ligne), et pour chaque menu, le menu complet et sa liste de courses.
 */
function usePrefetch(householdId: string, agenda: Agenda | undefined) {
  useEffect(() => {
    if (!agenda) return;
    for (const recipe of agenda.recipes) void setItem(`recipe:${householdId}:${recipe.slug}`, recipe);
    for (const weekStart of new Set(agenda.slots.map((s) => s.week_start))) {
      const path = { id: householdId, week_start: weekStart };
      void api.GET('/api/v1/households/{id}/menus/{week_start}', { params: { path } }).then(({ data }) => data && setItem(`menu:${householdId}:${weekStart}`, data)).catch(() => undefined);
      void api.GET('/api/v1/households/{id}/menus/{week_start}/shopping', { params: { path } }).then(({ data }) => data && setItem(`shopping:${householdId}:${weekStart}`, data)).catch(() => undefined);
    }
    void api.GET('/api/v1/households/{id}/menus', { params: { path: { id: householdId } } }).then(({ data }) => data && setItem(`menus:${householdId}`, data)).catch(() => undefined);
    void api.GET('/api/v1/households/{id}/members', { params: { path: { id: householdId } } }).then(({ data }) => data && setItem(`members:${householdId}`, data)).catch(() => undefined);
  }, [householdId, agenda]);
}

const HINT_KEY = 'chaudron.installHint';

/**
 * iPhone dans Safari, hors écran d'accueil : Safari efface au bout de quelques jours les données d'un site qu'on
 * n'a pas ajouté à l'écran d'accueil (la copie hors ligne avec). On le dit une fois.
 */
function InstallHint() {
  const [hidden, setHidden] = useState(() => {
    if (Platform.OS !== 'web' || typeof navigator === 'undefined') return true;
    const ios = /iPhone|iPad|iPod/.test(navigator.userAgent);
    const standalone = window.matchMedia?.('(display-mode: standalone)').matches || (navigator as { standalone?: boolean }).standalone;
    let dismissed = false;
    try {
      dismissed = localStorage.getItem(HINT_KEY) === '1';
    } catch {
      // Stockage indisponible : on montre le conseil.
    }
    return !ios || !!standalone || dismissed;
  });
  if (hidden) return null;
  return (
    <Card>
      <Body size={16}>Pour garder la semaine et les courses sans réseau sur iPhone : Partager, puis « Sur l’écran d’accueil ».</Body>
      <Row>
        <Button
          kind="quiet"
          label="Compris"
          onPress={() => {
            try {
              localStorage.setItem(HINT_KEY, '1');
            } catch {
              // Tant pis : il reviendra.
            }
            setHidden(true);
          }}
        />
      </Row>
    </Card>
  );
}

/**
 * Fraîcheur d'un plat : ce qui ne tiendra pas jusque-là (une ligne), un bouton pour tout congeler en rentrant, un
 * pour avancer le plat au dernier jour où tout est encore frais.
 */
function Freshness({ slot, warnings, today, busy, onFreeze, onMove }: { slot: MenuSlot; warnings: Warning[]; today: string; busy: boolean; onFreeze: (ingredient: string) => void; onMove: (request: MoveRequest) => void }) {
  if (!warnings.length) return null;
  const names = warnings.map((w) => w.ingredient_name ?? w.ingredient).join(', ');
  const bought = warnings[0].purchased;
  const advance = warnings.map((w) => w.advance_to).filter((d): d is string => !!d).sort()[0];
  return (
    <Stack gap={space[2]}>
      <NoticeLine notice={{ tone: 'warn', icon: 'fresh', text: `Ne tiendra pas jusque-là : ${names.toLowerCase()}${bought ? ` (courses du ${shortDateText(bought)})` : ''}` }} />
      <Row gap={space[1]}>
        <Button label="Congeler en rentrant" busy={busy} onPress={() => warnings.forEach((w) => w.ingredient && onFreeze(w.ingredient))} />
        {advance && advance >= today && advance < slot.date ? (
          <Button kind="quiet" label={`Avancer au ${shortDateText(advance)}`} onPress={() => onMove({ slot: slot.id, date: advance, meal: slot.meal, mode: 'insert' })} />
        ) : null}
      </Row>
    </Stack>
  );
}

/** Un plat de la liste : son résumé, « ⋯ » pour le déplacer, la fraîcheur ; suit le doigt pendant qu'on le glisse. */
function SlotBlock({ slot, warnings, actions, drag, handle }: { slot: MenuSlot; warnings: Warning[]; actions: SlotActions; drag: Drag | null; handle: ReactNode }) {
  const { palette, radius } = useTheme();
  const fresh = warnings.filter((w) => w.kind === 'freshness');
  const open = actions.panel === slot.id;
  return (
    <View
      style={[
        { gap: space[3] },
        drag ? { transform: [{ translateY: drag.dy }], zIndex: 10, opacity: 0.94, backgroundColor: palette.card, borderRadius: radius.card, boxShadow: '0 8px 24px rgba(0,0,0,0.25)' } : null,
      ]}
    >
      <SlotSummary
        slot={slot}
        warnings={warnings.filter((w) => w.kind !== 'freshness')}
        handle={handle}
        onMore={handle ? undefined : () => actions.setPanel(open ? null : slot.id)}
        moreOpen={open}
        actions={<Freshness slot={slot} warnings={fresh} today={actions.today} busy={actions.busy} onFreeze={(i) => actions.freeze(slot, i)} onMove={(r) => actions.move(r, slot.week_start)} />}
      />
      {open ? <MovePanel slot={slot} today={actions.today} occupied={(d, meal) => actions.occupied(d, meal, slot)} onMove={(r) => actions.move(r, slot.week_start)} onCancel={() => actions.setPanel(null)} busy={actions.busy} /> : null}
    </View>
  );
}

/** Le plat est lâché sur un repas déjà pris : intercaler, échanger ou servir ensemble. */
function DropChoice({ slot, date, today, taken, busy, onChoose, onCancel }: { slot: MenuSlot; date: string; today: string; taken: MenuSlot[]; busy: boolean; onChoose: (mode: 'insert' | 'swap' | 'together') => void; onCancel: () => void }) {
  return (
    <Stack gap={space[3]}>
      <Body size={16}>{`${splitTitle(slot.recipe.title).main} sur ${dayLabel(date, today).toLowerCase()} ${mealLabels[slot.meal]}, déjà pris par ${taken.map((s) => splitTitle(s.recipe.title).main).join(', ')} :`}</Body>
      <Stack gap={space[2]}>
        <Button kind="primary" label="Intercaler" busy={busy} onPress={() => onChoose('insert')} />
        <Body tone="muted" size={14}>{modeHelp.insert}</Body>
        <Row>
          <Button label="Échanger" onPress={() => onChoose('swap')} />
          <Button label="Même soir" onPress={() => onChoose('together')} />
          <Button kind="quiet" label="Annuler" onPress={onCancel} />
        </Row>
      </Stack>
    </Stack>
  );
}

