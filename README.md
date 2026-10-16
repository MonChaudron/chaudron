# Chaudron

Recettes et menus pour toute la maisonnée : un plat, plusieurs assiettes, des recettes qui s'adaptent à ton matériel,
la liste de courses qui suit, et la recette lue étape par étape sur ton Echo Show.

> En construction. Auto-hébergeable, léger (< 50 Mo de RAM), open source.

## Héberger Chaudron

Il faut Docker et Docker Compose. L'image (`ghcr.io/monchaudron/chaudron`) pèse environ 3 Mo, le serveur tourne avec
quelques Mo de RAM et PostgreSQL avec une trentaine.

```sh
mkdir -p chaudron/data/pg && cd chaudron
curl -fsSL -o docker-compose.yml https://raw.githubusercontent.com/MonChaudron/chaudron/main/deploy/docker-compose.yml
curl -fsSL -o .env https://raw.githubusercontent.com/MonChaudron/chaudron/main/deploy/.env.example
vi .env                    # changer le mot de passe : POSTGRES_PASSWORD et dans DATABASE_URL
docker compose up -d
curl http://localhost:28471/health   # {"status":"ok","database":"ok"}
```

- **Mot de passe** : le même dans `POSTGRES_PASSWORD` et dans `DATABASE_URL`
  (`postgres://chaudron:MOT_DE_PASSE@db:5432/chaudron`), en lettres et chiffres (`@ : / ? # %` cassent l'URL).
  Il est pris en compte **au premier démarrage seulement** : pour le changer ensuite, il faut repartir d'un
  `data/pg` vide (et donc perdre les données) ou le modifier dans PostgreSQL.
- **`data/pg`** : à créer avant le premier démarrage ; certains Docker (sur NAS notamment) ne créent pas le dossier
  d'un montage et échouent avec « Bind mount failed ».
- **Port** : 28471 par défaut, modifiable avec `CHAUDRON_PORT` dans `.env`.
- **Télécharger les fichiers sur la machine qui les exécute** (comme ci-dessus) plutôt que les copier depuis un autre
  poste : un éditeur peut changer leur encodage. Ils sont en ASCII pour limiter ce risque.
- **Connexion** : avec Google et/ou Amazon, à déclarer dans `.env` (voir `.env.example`) :
  - `PUBLIC_URL` : l'adresse publique de Chaudron (`https://chaudron.example.org`) ;
  - `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` : identifiant OAuth « Application Web » de la console Google Cloud,
    URL de retour `PUBLIC_URL/api/v1/auth/google/callback` ;
  - `AMAZON_CLIENT_ID` / `AMAZON_CLIENT_SECRET` : *Security Profile* Login with Amazon, *Allowed Return URL*
    `PUBLIC_URL/api/v1/auth/amazon/callback`.
- **Comptes** : la première personne qui se connecte crée son compte ; ensuite, les inscriptions sont fermées et on
  entre par un lien d'invitation (créé par un propriétaire du foyer). `SIGNUP_ALLOWED_EMAILS` les ouvre à une liste
  d'adresses, ou à tout le monde avec `*`. Une liste s'applique aussi au premier compte : sur une instance exposée
  à internet, y mettre son adresse Google évite qu'un inconnu se connecte avant soi.
- **Mise à jour** : `docker compose pull && docker compose up -d`.
- Avec l'ancien `docker-compose` (avec un tiret), remplacer `docker compose` par `docker-compose`.

## Développer

Le serveur tourne sans rien installer à part Docker : Rust tourne dans un conteneur jetable.

```sh
scripts/dev.sh up --build      # l'appli construite depuis les sources + PostgreSQL, sur http://localhost:28471
scripts/cargo.sh test          # cargo dans le conteneur, avec la base de dev (fmt, clippy, run… aussi)
scripts/dev.sh down            # arrêter ; down -v pour repartir de zéro
```

`scripts/dev.sh` crée `deploy/.env` à partir de `deploy/.env.example` s'il n'existe pas. En dev, la connexion passe
par un fournisseur factice (`AUTH_FAKE=1`) : on choisit un nom et une adresse, sans mot de passe ; les tests
l'utilisent aussi, sur un vrai port, pour dérouler la connexion OAuth de bout en bout. La base de dev est joignable
sur `localhost:5432`. Le contrat d'API est publié sur `/api/v1/openapi.json`.

### L'appli (Expo)

Pour travailler sur l'appli avec le rechargement à chaud, il faut Node 24 en local (le rechargement ne traverse pas
les montages Windows vers un conteneur) :

```sh
npm install                                                  # une fois
DEV_WEB_ORIGIN=http://localhost:8081 scripts/dev.sh up -d    # serveur et base, qui acceptent l'appli de :8081
npm run dev                                                  # l'appli sur http://localhost:8081
npm run typecheck && npm run lint && npm test                # comme la CI
```

**Contrat d'API** : le client TypeScript (`packages/api`) est généré depuis `packages/api/openapi.json`, lui-même écrit
par le serveur. Après un changement de l'API :

```sh
UPDATE_OPENAPI=1 scripts/cargo.sh test --test api            # réécrit openapi.json
npm run api:generate                                         # régénère les types
```

| Dossier | Contenu |
|---|---|
| `server/` | Serveur Rust (axum, sqlx, utoipa) : API, migrations ; sert l'appli construite |
| `apps/app/` | Appli Expo (Expo Router), web d'abord |
| `packages/api/` | Client TypeScript généré depuis l'OpenAPI |
| `packages/domain/` | Charte (tokens jour et nuit), libellés français, résumés du barème et de la présence |
| `deploy/` | docker-compose et exemple de configuration |
| `scripts/` | Raccourcis de dev |

## Licence

[AGPL-3.0](LICENSE). Polices Unbounded et Atkinson Hyperlegible Next sous SIL Open Font License 1.1
(`apps/app/public/fonts/OFL-*.txt`).
