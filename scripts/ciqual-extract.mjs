#!/usr/bin/env node
// Extrait compact de la table Ciqual (Anses, Licence Ouverte Etalab 2.0) pour server/data/ciqual.tsv.
//
// Usage : node scripts/ciqual-extract.mjs <dossier> <version> > server/data/ciqual.tsv
//   <dossier> : les XML téléchargés depuis https://doi.org/10.57745/RDMHWY, renommés alim.xml et compo.xml
//   <version> : ex. « 2025-11-03 » (date des fichiers) ; une nouvelle version est rechargée au démarrage du serveur.
//
// Teneurs pour 100 g : « - » → vide, « traces » → 0, « < x » → x (borne haute).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const [dir, version] = process.argv.slice(2);
if (!dir || !version) {
  console.error('usage : node scripts/ciqual-extract.mjs <dossier> <version>');
  process.exit(1);
}

// Constituants retenus, dans l'ordre des colonnes.
const constituents = [
  ['energy_kcal', '328'], // Énergie, règlement UE 1169/2011
  ['protein_g', '25000'],
  ['carbs_g', '31000'],
  ['sugars_g', '32000'],
  ['fat_g', '40000'],
  ['sat_fat_g', '40302'],
  ['fiber_g', '34100'],
  ['salt_g', '10004'],
];

const decode = (s) =>
  s
    .trim()
    .replaceAll('&apos;', "'")
    .replaceAll('&quot;', '"')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&amp;', '&');

function* records(file, tag) {
  const xml = readFileSync(join(dir, file), 'utf8');
  const re = new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`, 'g');
  for (const [, body] of xml.matchAll(re)) {
    const fields = {};
    for (const [, name, value] of body.matchAll(/<(\w+)>([^<]*)<\/\1>/g)) fields[name] = decode(value);
    yield fields;
  }
}

function amount(text) {
  if (text === undefined || text === '-' || text === '') return '';
  if (text === 'traces') return '0';
  const value = text.replace(/^<\s*/, '').replace(',', '.');
  if (!/^\d+(\.\d+)?$/.test(value)) throw new Error(`teneur illisible : ${text}`);
  return String(Number(value));
}

const wanted = new Map(constituents.map(([, code], i) => [code, i]));
const values = new Map();
for (const row of records('compo.xml', 'COMPO')) {
  const column = wanted.get(row.const_code);
  if (column === undefined) continue;
  if (!values.has(row.alim_code)) values.set(row.alim_code, Array(constituents.length).fill(''));
  values.get(row.alim_code)[column] = amount(row.teneur);
}

const lines = [];
for (const row of records('alim.xml', 'ALIM')) {
  const name = row.alim_nom_fr.replace(/\s+/g, ' ');
  if (/[\t\n]/.test(name)) throw new Error(`nom illisible : ${name}`);
  lines.push([
    Number(row.alim_code),
    name,
    row.alim_grp_code,
    row.alim_ssgrp_code,
    ...(values.get(row.alim_code) ?? Array(constituents.length).fill('')),
  ]);
}
lines.sort((a, b) => a[0] - b[0]);

process.stdout.write(
  [
    `# Ciqual ${version} : Anses, Table de composition nutritionnelle des aliments Ciqual, Licence Ouverte Etalab 2.0.`,
    '# Extrait par scripts/ciqual-extract.mjs. Teneurs pour 100 g ; vide : non renseigné.',
    `version\t${version}`,
    ['code', 'name', 'group', 'subgroup', ...constituents.map(([name]) => name)].join('\t'),
    ...lines.map((l) => l.join('\t')),
  ].join('\n') + '\n',
);
