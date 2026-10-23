import { errorMessage, type PriceIssue } from '@chaudron/api';
import { formatPrice, ingredientStates, space } from '@chaudron/domain';
import { useState } from 'react';
import { View } from 'react-native';

import { api } from '../lib/api';
import { useLoad } from '../lib/useLoad';
import { Body, Button, Card, ErrorText, Field, Row, Stack, Title } from './index';

/** Au-delà, les ingrédients sans prix se replient : une instance neuve en a beaucoup avant son premier passage. */
const SHOWN = 12;

const kinds: Record<PriceIssue['kind'], string> = {
  divergent: 'Open Prices et l’INSEE s’écartent : le rattachement est sans doute faux',
  unlinked: 'Utilisé par une recette, sans rattachement',
  missing: 'Rattaché, sans prix trouvé',
};

const when = (iso: string) => new Intl.DateTimeFormat('fr-FR', { day: 'numeric', month: 'long', hour: '2-digit', minute: '2-digit' }).format(new Date(iso));

/** À revoir › Prix : corriger les rattachements, jamais les prix. */
export function PricesAdmin() {
  const review = useLoad(() => api.GET('/api/v1/community/prices'), []);
  const [error, setError] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  const refresh = async () => {
    const { response, error: failure } = await api.POST('/api/v1/community/prices/refresh');
    setError(response.ok ? null : errorMessage(failure));
    setTimeout(() => void review.reload(), 3000);
  };
  const data = review.data;
  const issues = data?.issues ?? [];
  const shown = all ? issues : issues.slice(0, SHOWN);
  const run = data?.last_run;
  return (
    <Card style={{ maxWidth: 820 }}>
      <Title level={2}>{`Prix (${issues.length})`}</Title>
      <Body tone="muted" size={15}>
        {run
          ? `Dernière mise à jour : ${when(run.started_at)}, ${run.finished_at ? (run.ok ? `réussie, ${run.priced} prix` : 'en échec') : 'en cours'}.`
          : 'Pas encore de mise à jour des prix.'}
      </Body>
      {run && !run.ok && run.message ? <Body tone="alert" size={14}>{run.message}</Body> : null}
      <Row>
        <Button kind="quiet" label="Relancer la mise à jour" onPress={() => void refresh()} />
      </Row>
      <ErrorText>{error ?? review.error}</ErrorText>
      <View style={{ gap: space[2] }}>
        {shown.map((issue) => (
          <IssueRow key={`${issue.ingredient_id}-${issue.state}`} issue={issue} onSaved={() => void review.reload()} />
        ))}
      </View>
      {issues.length > SHOWN ? <Button kind="quiet" label={all ? 'Replier' : `Voir les ${issues.length - SHOWN} autres`} onPress={() => setAll(!all)} /> : null}
    </Card>
  );
}

function IssueRow({ issue, onSaved }: { issue: PriceIssue; onSaved: () => void }) {
  const [open, setOpen] = useState(false);
  const [off, setOff] = useState(issue.link?.off_category ?? '');
  const [product, setProduct] = useState(issue.link?.off_product_category ?? '');
  const [series, setSeries] = useState(issue.link?.insee_series ?? '');
  const [index, setIndex] = useState(issue.link?.insee_index ?? '');
  const [piece, setPiece] = useState('');
  const [error, setError] = useState<string | null>(null);
  const prices = [issue.cents != null ? `Open Prices ${formatPrice(issue.cents)}` : null, issue.insee_cents != null ? `INSEE ${formatPrice(issue.insee_cents)}` : null].filter(Boolean).join(' · ');
  const save = async () => {
    const { response, error: failure } = await api.PUT('/api/v1/community/ingredients/{id}/price-links/{state}', {
      params: { path: { id: issue.ingredient_id, state: issue.state } },
      body: {
        off_category: off || null,
        off_product_category: product || null,
        insee_series: series || null,
        insee_per: issue.link?.insee_per ?? null,
        insee_index: index || null,
        piece_g: piece.trim() ? Number(piece.replace(',', '.')) : null,
      },
    });
    if (!response.ok) return setError(errorMessage(failure));
    setOpen(false);
    onSaved();
  };
  return (
    <Stack gap={space[1]}>
      <Row gap={space[3]}>
        <View style={{ flex: 1, gap: 2 }}>
          <Body size={16}>{`${issue.name}${issue.state !== 'fresh' ? ` (${ingredientStates[issue.state].toLowerCase()})` : ''}`}</Body>
          <Body tone="muted" size={14}>{[kinds[issue.kind], prices].filter(Boolean).join(' · ')}</Body>
        </View>
        <Button kind="quiet" label={open ? 'Fermer' : 'Corriger'} onPress={() => setOpen(!open)} />
      </Row>
      {open ? (
        <Stack gap={space[2]}>
          <Field label="Catégorie Open Food Facts, en vrac" value={off} onChangeText={setOff} placeholder="en:carrots" autoCapitalize="none" />
          <Field label="Catégorie Open Food Facts, produits à code-barres" value={product} onChangeText={setProduct} placeholder="en:gnocchi" autoCapitalize="none" />
          <Field label="Série INSEE (idbank du prix moyen)" value={series} onChangeText={setSeries} placeholder="000641422" />
          <Field label="Indice INSEE du poste (idbank)" value={index} onChangeText={setIndex} placeholder="011815033" />
          <Field label="Poids d’une pièce, en grammes" hint="Vide : inchangé." value={piece} onChangeText={setPiece} keyboardType="decimal-pad" />
          <ErrorText>{error}</ErrorText>
          <Row>
            <Button kind="primary" label="Enregistrer et recalculer" onPress={() => void save()} />
          </Row>
        </Stack>
      ) : null}
    </Stack>
  );
}
