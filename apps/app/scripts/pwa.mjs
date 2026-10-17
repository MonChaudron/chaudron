// Après `expo export -p web` : inscrit dans dist/sw.js la liste des fichiers de l'appli et une version tirée de leur
// contenu. Un build qui change un fichier change donc le service worker, et le navigateur installe la nouvelle version.
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const dist = path.resolve(import.meta.dirname, '../dist');
const skip = new Set(['sw.js', 'metadata.json', 'index.html']);

function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    return entry.isDirectory() ? walk(full) : [full];
  });
}

const files = walk(dist)
  .map((file) => path.relative(dist, file).split(path.sep).join('/'))
  .filter((file) => !skip.has(file) && !file.endsWith('.map'))
  .sort();
const hash = createHash('sha256');
for (const file of [...files, 'index.html']) {
  hash.update(file);
  hash.update(fs.readFileSync(path.join(dist, file)));
}
const version = hash.digest('hex').slice(0, 12);
const target = path.join(dist, 'sw.js');
const source = fs.readFileSync(target, 'utf8');
if (!source.includes("'__VERSION__'") || !source.includes('[] /* __ASSETS__ */')) throw new Error('dist/sw.js : gabarit inattendu');
fs.writeFileSync(
  target,
  source.replace("'__VERSION__'", JSON.stringify(version)).replace('[] /* __ASSETS__ */', JSON.stringify(files.map((f) => `/${f}`))),
);
console.log(`sw.js : version ${version}, ${files.length} fichiers`);
