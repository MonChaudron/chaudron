# Chaudron

Recettes et menus pour toute la maisonnée : un plat, plusieurs assiettes, des recettes qui s'adaptent à ton matériel,
la liste de courses qui suit, et la recette lue étape par étape sur ton Echo Show.

> En construction. Auto-hébergeable, léger (< 50 Mo de RAM), open source.

## Héberger Chaudron

Docker et Docker Compose suffisent. Dans un dossier, récupérer `deploy/docker-compose.yml` et `deploy/.env.example`,
copier `.env.example` en `.env` en changeant le mot de passe, puis :

```sh
docker compose up -d
```

Chaudron répond sur le port 8090 (`/health` pour vérifier). L'image (`ghcr.io/monchaudron/chaudron`) pèse environ
3 Mo et le serveur tourne avec quelques Mo de RAM ; PostgreSQL en prend une trentaine.

## Développer

Rien à installer à part Docker : Rust tourne dans un conteneur jetable.

```sh
scripts/dev.sh up --build      # l'appli construite depuis les sources + PostgreSQL, sur http://localhost:8090
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
