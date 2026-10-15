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
- **Mise à jour** : `docker compose pull && docker compose up -d`.
- Avec l'ancien `docker-compose` (avec un tiret), remplacer `docker compose` par `docker-compose`.

## Développer

Rien à installer à part Docker : Rust tourne dans un conteneur jetable.

```sh
scripts/dev.sh up --build      # l'appli construite depuis les sources + PostgreSQL, sur http://localhost:28471
scripts/cargo.sh test          # cargo dans le conteneur, avec la base de dev (fmt, clippy, run… aussi)
scripts/dev.sh down            # arrêter ; down -v pour repartir de zéro
```

`scripts/dev.sh` crée `deploy/.env` à partir de `deploy/.env.example` s'il n'existe pas. La base de dev est joignable
sur `localhost:5432`. Le contrat d'API est publié sur `/api/v1/openapi.json`.

| Dossier | Contenu |
|---|---|
| `server/` | Serveur Rust (axum, sqlx, utoipa) : API, migrations, fichiers web |
| `deploy/` | docker-compose et exemple de configuration |
| `scripts/` | Raccourcis de dev |

## Licence

[AGPL-3.0](LICENSE).
