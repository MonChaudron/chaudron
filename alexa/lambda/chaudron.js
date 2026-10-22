// Client de l'API Chaudron, avec le jeton de la liaison de compte. `https` de Node : la Lambda hébergée est en
// Node 16, sans fetch.
const http = require('http');
const https = require('https');

/** La liaison est absente, expirée ou retirée : il faut lier le compte de nouveau. */
class NonLie extends Error {}

/** Appel HTTP en JSON ; renvoie `{ status, body }` (body : `null` sans contenu). */
function appel(base, jeton, methode, chemin, corps) {
  const url = new URL(`/api/v1${chemin}`, base);
  const module = url.protocol === 'http:' ? http : https;
  const donnees = corps === undefined ? null : JSON.stringify(corps);
  return new Promise((resolve, reject) => {
    const req = module.request(
      url,
      {
        method: methode,
        timeout: 6000,
        headers: {
          authorization: `Bearer ${jeton}`,
          accept: 'application/json',
          'user-agent': 'chaudron-alexa',
          ...(donnees ? { 'content-type': 'application/json', 'content-length': Buffer.byteLength(donnees) } : {}),
        },
      },
      (res) => {
        const morceaux = [];
        res.on('data', (m) => morceaux.push(m));
        res.on('end', () => {
          const texte = Buffer.concat(morceaux).toString('utf8');
          let body = null;
          try {
            body = texte ? JSON.parse(texte) : null;
          } catch {
            body = texte;
          }
          if (res.statusCode === 401) return reject(new NonLie('jeton refusé'));
          if (res.statusCode >= 400) {
            const err = new Error(`${methode} ${chemin} : ${res.statusCode} ${texte.slice(0, 200)}`);
            err.status = res.statusCode;
            return reject(err);
          }
          resolve({ status: res.statusCode, body });
        });
      },
    );
    req.on('timeout', () => req.destroy(new Error(`${methode} ${chemin} : délai dépassé`)));
    req.on('error', reject);
    if (donnees) req.write(donnees);
    req.end();
  });
}

/** Client lié à un jeton ; `foyer()` apprend le foyer du jeton (Amazon ne passe que le jeton). */
function client(base, jeton) {
  const get = (chemin) => appel(base, jeton, 'GET', chemin).then((r) => r.body);
  return {
    foyer: () => get('/token').then((t) => t.household_id),
    agenda: (foyer, depuis, jours) => get(`/households/${foyer}/agenda?from=${depuis}&days=${jours}`),
    /** La cuisson en cours, ou `null`. */
    cuisson: (foyer) => get(`/households/${foyer}/cooking`),
    cuisiner: (foyer, place) => appel(base, jeton, 'PUT', `/households/${foyer}/cooking`, place).then((r) => r.body),
    terminer: (foyer) => appel(base, jeton, 'DELETE', `/households/${foyer}/cooking`),
    /** Une note du cahier à l'étape en cours ; `null` sans cuisson en cours. */
    remarquer: (foyer, texte) =>
      appel(base, jeton, 'POST', `/households/${foyer}/cooking/remark`, { text: texte })
        .then((r) => r.body)
        .catch((err) => {
          if (err.status === 409) return null;
          throw err;
        }),
  };
}

module.exports = { client, NonLie };
