# Skill Alexa

« Alexa, ouvre le chaudron » : le menu et les recettes du foyer, lus étape par étape, avec l'écran de l'Echo Show.
La skill appelle l'API de l'instance Chaudron avec le jeton de la liaison de compte ; la place dans la recette vit sur
le serveur (`/cooking`), on la retrouve en revenant et le téléphone la partage.

| Dossier | Contenu |
|---|---|
| `lambda/` | Code Node.js (ASK SDK) : `index.js` (intentions), `parole.js` (ce qui se dit), `ecran.js` et `apl.json` (l'écran), `chaudron.js` (client de l'API) |
| `lambda/test/` | Tests (`npm test -w @chaudron/alexa`), avec une fausse API en mémoire |
| `skill-package/` | Modèle d'interaction fr-FR |

Le code tourne sur Node 16 (Lambda Alexa-hosted) : `https` plutôt que `fetch`.

## Ce qu'on dit

| Où | Dire |
|---|---|
| Ouverture | « Alexa, ouvre le chaudron » : la cuisson en cours, sinon le plat de ce soir, sinon le menu |
| Menu | « le menu », « le curry », « celle de demain », « on mange quoi jeudi » |
| Recette | « on commence », « les ingrédients », « comment je sers », « je n'ai pas le Cookeo » |
| Cuisine | « Alexa, suivant / précédent / répète / étape 3 », « détaille », « accélère », « lance le minuteur » |
| Musique | « mets la radio », « Alexa, pause » : la skill rend la main à Alexa ; « Alexa, ouvre le chaudron » pour reprendre |

L'écran est tactile : plats du menu, *Commencer*, *Suivante*, *Précédente*, liste des étapes.

## Installer pour son instance

1. Créer une skill personnalisée fr-FR, hébergée (« Alexa-hosted », Node.js), nom d'invocation « le chaudron ».
2. Pousser `lambda/` et `skill-package/` dans le dépôt de la skill, avec un `lambda/config.json` :
   `{ "api_url": "https://chaudron.example.org" }` (l'adresse publique de l'instance, sans `/` final).
3. Modèle : `skill-package/interactionModels/custom/fr-FR.json`.
4. Liaison de compte (Account Linking), « Auth Code Grant » :
   - Web Authorization URI : `https://chaudron.example.org/oauth/authorize`
   - Access Token URI : `https://chaudron.example.org/api/v1/oauth/token`, authentification HTTP Basic
   - Client ID et secret : ceux de `ALEXA_CLIENT_ID` et `ALEXA_CLIENT_SECRET` dans le `.env` de l'instance, avec les
     « Alexa Redirect URLs » de la console dans `ALEXA_REDIRECT_URIS` (voir `deploy/.env.example`).
5. Permission « Timers » (minuteurs) dans le manifeste ; l'interface APL activée.
6. Dans l'appli Alexa : activer la skill, lier le compte Chaudron.
