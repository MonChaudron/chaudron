// Page d'accueil hors connexion : ce qu'est Chaudron, pour la famille et les curieux d'abord, puis l'auto-hébergement.
// Web seulement : du HTML et du CSS (requêtes de conteneur, filtre SVG de la craie) que React Native ne sait pas
// exprimer. Couleurs et polices du thème, en variables CSS : le mode nuit et les quatre chartes suivent.
import type { SignupInfo } from '@chaudron/api';
import { inviteToken } from '@chaudron/domain';
import { Stack as Router, useRouter } from 'expo-router';
import { type CSSProperties, type FormEvent, type ReactNode, useEffect, useState } from 'react';

import { api } from '../../lib/api';
import { useSession } from '../../lib/session';
import { useTheme } from '../../lib/theme';

const REPO = 'https://github.com/MonChaudron/chaudron';

const providerNames: Record<string, string> = { google: 'Google', amazon: 'Amazon', fake: 'la connexion factice' };

export function Landing() {
  const { palette, fonts } = useTheme();
  const { providers } = useSession();
  const router = useRouter();
  const [signup, setSignup] = useState<SignupInfo['signup'] | null>(null);
  useEffect(() => {
    let live = true;
    api
      .GET('/api/v1/auth/signup')
      .then(({ data }) => live && data && setSignup(data.signup))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);

  // Tant qu'on ne sait pas, on propose de créer un foyer : c'est le cas le plus courant.
  const canCreate = signup !== 'invite';
  const login = providers.map((p) => providerNames[p.id] ?? p.id);
  const loginText = login.length ? `Connexion avec ${login.join(' ou ')}.` : null;

  const vars = {
    '--bg': palette.bg,
    '--card': palette.card,
    '--text': palette.text,
    '--muted': palette.muted,
    '--line': palette.line,
    '--action': palette.action,
    '--on-action': palette.onAction,
    '--action-border': palette.actionBorder === 'transparent' ? palette.action : palette.actionBorder,
    '--link': palette.link,
    '--chip': palette.chip,
    '--title-font': `'${fonts.title}'`,
    '--text-font': `'${fonts.text}'`,
  } as CSSProperties;

  const go = (id: string) => (event: { preventDefault: () => void }) => {
    event.preventDefault();
    document.getElementById(id)?.scrollIntoView({ behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });
  };

  return (
    <div className="acc" style={vars}>
      <Router.Screen options={{ title: 'Chaudron · le menu de la semaine pour toute la maison' }} />
      <style>{css}</style>
      <ChalkFilter />

      <div className="acc-wrap">
        <nav className="acc-nav" aria-label="Chaudron">
          <span className="acc-logo">chaudrön</span>
          <div className="acc-links">
            <a href="#semaine" onClick={go('semaine')}>Comment ça marche</a>
            <a href="#heberger" onClick={go('heberger')}>Héberger</a>
            <a className="acc-btn acc-ghost acc-small" href="/connexion" onClick={(e) => (e.preventDefault(), router.push('/connexion'))}>
              Se connecter
            </a>
          </div>
        </nav>

        <header className="acc-hero">
          <Slate />
          <div className="acc-pitch">
            <h1>Le menu s’écrit, la maison suit.</h1>
            <p>
              Chaudron fait le menu de la semaine pour toute la maison : les portions de chacun, ce qu’on prépare la
              veille, la liste de courses partagée. Le soir, suis la recette pas à pas sur ton téléphone, ou demande-la à
              ton enceinte Alexa.
            </p>
            <div className="acc-ctas">
              {canCreate ? (
                <a className="acc-btn acc-primary" href="/connexion" onClick={(e) => (e.preventDefault(), router.push('/connexion'))}>
                  Créer notre foyer
                </a>
              ) : null}
              <a className={`acc-btn ${canCreate ? 'acc-ghost' : 'acc-primary'}`} href="#rejoindre" onClick={go('rejoindre')}>
                J’ai une invitation
              </a>
            </div>
            {loginText ? <p className="acc-note">{loginText}</p> : null}
          </div>
        </header>
      </div>

      <section className="acc-how" id="semaine">
        <div className="acc-wrap">
          <h2 className="acc-h2">Une semaine avec Chaudron</h2>
          <p className="acc-lead">Un plat, plusieurs assiettes : chacun mange à sa faim, et rien ne se perd.</p>
          <div className="acc-days">
            <Moment when="le week-end" title="On écrit le menu" example={<><b>Inès</b> mange à la cantine mardi : une boîte de moins.</>}>
              Choisis les plats ; Chaudron calcule les quantités pour chacun, selon qui est là.
            </Moment>
            <Moment when="au magasin" title="On fait les courses" example={<>✓ Carottes 1,4 kg, <span className="acc-muted">prises par Marc</span></>}>
              La liste se range par rayon et se coche à plusieurs, en direct, même sans réseau.
            </Moment>
            <Moment when="la veille" title="On prend de l’avance" example="Pour demain : mariner le bœuf ce soir.">
              Ce qui se prépare la veille, ce qui sort du congélateur : rappelé au bon moment.
            </Moment>
            <Moment when="le soir" title="On cuisine" example="« Alexa, étape suivante. »">
              La recette pas à pas, avec ton matériel : sur le téléphone, ou à voix haute avec Alexa.
            </Moment>
          </div>
        </div>
      </section>

      <div className="acc-wrap">
        <section className="acc-app" aria-labelledby="acc-app-title">
          <div>
            <h2 className="acc-h2" id="acc-app-title">Pensé pour ta cuisine, pas pour une autre.</h2>
            <p className="acc-lead">Chaudron connaît ta maison : qui mange quoi, et avec quel matériel.</p>
            <div className="acc-points">
              <Point icon={icons.people} title="Les portions de chacun">
                Un ado qui mange double, un enfant qui mange moins, les boîtes du lendemain midi : les quantités suivent.
              </Point>
              <Point icon={icons.pot} title="La recette qui va avec ton matériel">
                Cookeo, four, Thermomix ou une simple poêle : chaque plat a la façon de faire qui te convient.
              </Point>
              <Point icon={icons.dislike} title="Les goûts de chacun">
                Allergies et « je n’aime pas » sont signalés dès que tu composes le menu.
              </Point>
              <Point icon={icons.phone} title="Partout dans la maison">
                Sur le téléphone, même hors ligne ; sur l’ordinateur ; à voix haute avec Alexa.
              </Point>
            </div>
          </div>
          <Phones />
        </section>

        <section className="acc-join" id="rejoindre">
          {canCreate ? (
            <div className="acc-card acc-accent">
              <div className="acc-hand">nouveau dans la cuisine ?</div>
              <h3>Crée ton foyer</h3>
              <p>Une minute pour dire qui mange à la maison et ce que vous avez comme matériel. Ensuite, le premier menu.</p>
              {signup === 'allowlist' ? <p className="acc-small-print">Sur cette instance, seules quelques adresses peuvent créer un foyer ; les autres entrent par invitation.</p> : null}
              <a className="acc-btn acc-dark" href="/connexion" onClick={(e) => (e.preventDefault(), router.push('/connexion'))}>
                Créer notre foyer
              </a>
            </div>
          ) : (
            <div className="acc-card acc-accent">
              <div className="acc-hand">une cuisine privée</div>
              <h3>Sur invitation</h3>
              <p>Ce Chaudron est réservé à ses foyers : on y entre par un lien d’invitation. Pour avoir le tien, héberge ton propre Chaudron, juste en dessous.</p>
              <a className="acc-btn acc-dark" href="#heberger" onClick={go('heberger')}>
                Héberger Chaudron
              </a>
            </div>
          )}
          <Invite onOpen={(token) => router.push(`/invitation/${token}`)} />
        </section>
      </div>

      <section className="acc-host" id="heberger">
        <div className="acc-wrap acc-host-grid">
          <div>
            <div className="acc-hand">pour les bricoleurs</div>
            <h2>Ta cuisine, ton serveur.</h2>
            <p>
              Chaudron est un logiciel libre. Installe-le sur ton NAS ou un Raspberry Pi : tes recettes et tes menus restent
              chez toi, et tu invites qui tu veux.
            </p>
            <div className="acc-facts">
              <div><b>3 Mo</b><span>l’image Docker</span></div>
              <div><b>&lt; 50 Mo</b><span>de RAM, base comprise</span></div>
              <div><b>AGPL</b><span>code ouvert</span></div>
            </div>
          </div>
          <div className="acc-term" aria-label="Installation">
            <div className="acc-dots" aria-hidden="true"><i /><i /><i /></div>
            <code>
              <span className="acc-p">$</span> mkdir -p chaudron/data/pg &amp;&amp; cd chaudron
              <br />
              <span className="acc-p">$</span> curl -fsSL -o docker-compose.yml {`${REPO.replace('github.com', 'raw.githubusercontent.com')}/main/deploy/docker-compose.yml`}
              <br />
              <span className="acc-p">$</span> curl -fsSL -o .env {`${REPO.replace('github.com', 'raw.githubusercontent.com')}/main/deploy/.env.example`}
              <br />
              <span className="acc-c"># choisir un mot de passe dans .env, puis :</span>
              <br />
              <span className="acc-p">$</span> docker compose up -d
            </code>
            <a href={`${REPO}#héberger-chaudron`} target="_blank" rel="noreferrer">Le guide d’installation et le code sur GitHub</a>
          </div>
        </div>
      </section>
      <footer className="acc-footer">
        <div className="acc-wrap">
          <span className="acc-logo acc-logo-small">chaudrön</span>
          <span>
            Logiciel libre (AGPL-3.0)
            <a href={REPO} target="_blank" rel="noreferrer">GitHub</a>
            <a href="/connexion" onClick={(e) => (e.preventDefault(), router.push('/connexion'))}>Se connecter</a>
          </span>
        </div>
      </footer>
    </div>
  );
}

/** Le menu d'une semaine écrit à la craie sur une vraie ardoise. Un exemple : la famille Martin n'existe pas. */
function Slate() {
  return (
    <figure className="acc-slate" aria-label="Exemple : le menu de la semaine écrit à la craie sur une ardoise">
      <img src="/accueil/ardoise-1200.webp" srcSet="/accueil/ardoise-900.webp 900w, /accueil/ardoise-1200.webp 1200w" sizes="(max-width: 640px) 100vw, 560px" alt="" />
      <div className="acc-chalk" aria-hidden="true">
        <div className="acc-who">Cette semaine chez les Martin</div>
        <div className="acc-title">Menu de la maison</div>
        <div className="acc-menu">
          <b>dim.</b><span>Sauté de porc au cidre</span>
          <b>lun.</b><span>Wok de bœuf <small>(+ boîtes)</small></span>
          <b>mar.</b><span>Blanquette de dinde</span>
          <b>mer.</b><span>Omelette épaisse <small>— express</small></span>
          <b>jeu.</b><span>Lentilles tièdes, saumon</span>
          <b className="acc-extra">ven.</b><span className="acc-extra">Poulet rôti, grenailles</span>
        </div>
        <div className="acc-rule" />
        <div className="acc-foot"><span>6 plats · 36 courses</span><span>4 à table</span></div>
      </div>
    </figure>
  );
}

/** Craie : le trait se ronge un peu (bruit fin) et tremble à peine ; reste lisible. */
function ChalkFilter() {
  return (
    <svg width="0" height="0" style={{ position: 'absolute' }} aria-hidden="true">
      <defs>
        <filter id="acc-chalk" x="-5%" y="-20%" width="110%" height="140%">
          <feTurbulence type="fractalNoise" baseFrequency="1.7" numOctaves={2} seed={3} result="grain" />
          <feColorMatrix in="grain" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 -1.5 1.55" result="holes" />
          <feComposite in="SourceGraphic" in2="holes" operator="in" result="eaten" />
          <feTurbulence type="fractalNoise" baseFrequency="0.04" numOctaves={1} seed={8} result="wobble" />
          <feDisplacementMap in="eaten" in2="wobble" scale={1.2} xChannelSelector="R" yChannelSelector="G" result="rough" />
          <feGaussianBlur in="rough" stdDeviation={0.25} result="soft" />
          <feMerge>
            <feMergeNode in="soft" />
            <feMergeNode in="rough" />
          </feMerge>
        </filter>
      </defs>
    </svg>
  );
}

function Moment({ when, title, example, children }: { when: string; title: string; example: ReactNode; children: ReactNode }) {
  return (
    <article className="acc-day">
      <div className="acc-hand acc-when">{when}</div>
      <h3>{title}</h3>
      <p>{children}</p>
      <div className="acc-mini">{example}</div>
    </article>
  );
}

function Point({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <div className="acc-point">
      <div className="acc-ic" aria-hidden="true">{icon}</div>
      <div>
        <h3>{title}</h3>
        <p>{children}</p>
      </div>
    </div>
  );
}

/** Coller le lien d'invitation reçu : il ouvre la page de l'invitation. */
function Invite({ onOpen }: { onOpen: (token: string) => void }) {
  const [text, setText] = useState('');
  const [error, setError] = useState(false);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const token = inviteToken(text);
    if (token) onOpen(token);
    else setError(true);
  };
  return (
    <form className="acc-card" onSubmit={submit}>
      <div className="acc-hand">on t’a invité ?</div>
      <h3>Rejoins ta famille</h3>
      <p>Ouvre le lien reçu par message : il te fait entrer dans le foyer. Tu peux aussi le coller ici.</p>
      <label className="acc-field">
        <span>Lien d’invitation</span>
        <input
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setError(false);
          }}
          placeholder="https://…/invitation/…"
          autoComplete="off"
          aria-invalid={error}
        />
      </label>
      {error ? <p className="acc-error" role="alert">Ce n’est pas un lien d’invitation. Copie le lien entier reçu par message.</p> : null}
      <button className="acc-btn acc-ghost" type="submit">Ouvrir l’invitation</button>
      <p className="acc-small-print">Un lien vaut 7 jours. Expiré ? Demande-en un nouveau à qui t’a invité.</p>
    </form>
  );
}

/** Deux écrans de l'appli, en exemple (famille fictive). */
function Phones() {
  return (
    <div className="acc-phones" aria-hidden="true">
      <div className="acc-phone acc-phone-1">
        <div className="acc-screen">
          <div className="acc-scr-top"><span className="acc-logo-xs">chaudrön</span><span className="acc-ava">CM</span></div>
          <div className="acc-scr-title">Cette semaine</div>
          <div className="acc-strip">
            {[['M', 21], ['J', 22], ['V', 23], ['S', 24], ['D', 25], ['L', 26], ['M', 27]].map(([d, n], i) => (
              <div key={i} className={i === 0 ? 'on' : undefined}>{d}<b>{n}</b></div>
            ))}
          </div>
          <div className="acc-tonight">
            <div className="acc-xs-strong">Ce soir</div>
            <div className="acc-vg">{icons.pot}</div>
            <div className="acc-scr-dish">Dahl de lentilles</div>
            <div className="acc-xs">butternut et poulet</div>
            <div className="acc-xs acc-xs-mid">Cookeo · 20 min · 6 portions, dont 2 en boîtes</div>
            <div className="acc-warn">⊘ Léo n’aime pas : coriandre</div>
            <div className="acc-scr-btns"><span className="dark">Voir la recette</span><span>Déplacer…</span></div>
          </div>
          <div className="acc-tab"><span className="act">Semaine</span><span>Menus</span><span>Courses</span><span>Recettes</span><span>Foyer</span></div>
        </div>
      </div>
      <div className="acc-phone acc-phone-2">
        <div className="acc-screen">
          <div className="acc-scr-top"><span className="acc-logo-xs">chaudrön</span><span className="acc-ava">CM</span></div>
          <div className="acc-scr-title">Courses</div>
          <div className="acc-xs">Menu d’automne, du dim. 18 au ven. 23</div>
          <div className="acc-progress"><div /></div>
          <div className="acc-xs">15 sur 36 pris, 21 à prendre</div>
          <div className="acc-list">
            <div className="acc-scr-aisle">Fruits et légumes</div>
            <Row done label="Ail" qty="3" />
            <Row done label="Brocoli" qty="400 g" />
            <Row label="Butternut" detail="Dahl de lentilles" qty="1,2 kg" />
            <Row label="Carotte" detail="Sauté de porc et 2 autres" qty="1,4 kg" />
            <Row label="Champignon" qty="1,2 kg" last />
          </div>
          <div className="acc-tab"><span>Semaine</span><span>Menus</span><span className="act">Courses</span><span>Recettes</span><span>Foyer</span></div>
        </div>
      </div>
    </div>
  );
}

function Row({ label, detail, qty, done = false, last = false }: { label: string; detail?: string; qty: string; done?: boolean; last?: boolean }) {
  return (
    <div className={`acc-row${last ? ' last' : ''}`}>
      <span className={`acc-ck${done ? ' on' : ''}`} />
      <span className={done ? 'acc-done' : undefined}>
        {label}
        {detail ? <><br /><span className="acc-xxs">{detail}</span></> : null}
      </span>
      <span className="acc-q">{qty}</span>
    </div>
  );
}

const stroke = { fill: 'none', stroke: 'currentColor', strokeWidth: 2, strokeLinecap: 'round', strokeLinejoin: 'round' } as const;
const icons = {
  people: (
    <svg width="22" height="22" viewBox="0 0 24 24" {...stroke}>
      <circle cx="9" cy="8" r="3.5" />
      <path d="M3 20c.8-3.5 3.2-5.5 6-5.5s5.2 2 6 5.5M16 4.5a3.5 3.5 0 0 1 0 7M18.5 14.5c1.4.8 2.2 2.6 2.5 5.5" />
    </svg>
  ),
  pot: (
    <svg width="22" height="22" viewBox="0 0 24 24" {...stroke}>
      <path d="M4.5 10.5h15v5.5a4 4 0 0 1-4 4h-7a4 4 0 0 1-4-4z" />
      <path d="M3.5 10.5h17M10 8h4M9 5.5c0-1 .8-1 .8-2M14 5.5c0-1 .8-1 .8-2" />
    </svg>
  ),
  dislike: (
    <svg width="22" height="22" viewBox="0 0 24 24" {...stroke}>
      <circle cx="12" cy="12" r="8.5" />
      <line x1="6" y1="18" x2="18" y2="6" />
    </svg>
  ),
  phone: (
    <svg width="22" height="22" viewBox="0 0 24 24" {...stroke}>
      <rect x="6" y="2.5" width="12" height="19" rx="3" />
      <line x1="10.5" y1="18" x2="13.5" y2="18" />
    </svg>
  ),
};

const css = `
.acc { position: fixed; inset: 0; overflow-y: auto; overflow-x: hidden; background: var(--bg); color: var(--text); font-family: var(--text-font), system-ui, sans-serif; -webkit-font-smoothing: antialiased; }
.acc *, .acc *::before, .acc *::after { box-sizing: border-box; }
.acc a:not(.acc-btn) { color: inherit; }
.acc :focus-visible { outline: 3px solid var(--link); outline-offset: 3px; border-radius: 8px; }
.acc-wrap { max-width: 1160px; margin: 0 auto; padding: 0 40px; }
.acc-hand { font-family: 'Caveat', cursive; }
.acc-muted { color: var(--muted); }
.acc-logo { font-family: 'Unbounded', var(--title-font), sans-serif; font-weight: 700; font-size: 26px; }
.acc-logo-small { font-size: 18px; color: #EEF3DC; }
.acc-btn { display: inline-block; border-radius: 16px; padding: 14px 22px; font: 600 16px/1.2 var(--text-font), sans-serif; text-decoration: none; cursor: pointer; border: 1.5px solid var(--line); background: var(--card); color: var(--text); }
.acc-btn:hover { filter: brightness(.97); }
.acc-primary { background: var(--action); color: var(--on-action); border: 2px solid var(--action-border); }
.acc-dark { background: var(--text); color: var(--card); border: 2px solid var(--text); }
.acc-ghost { background: var(--card); }
.acc-small { padding: 10px 18px; }
.acc-h2 { font-family: var(--title-font), sans-serif; font-weight: 700; font-size: 34px; line-height: 1.1; margin: 0 0 10px; }
.acc-lead { color: var(--muted); font-size: 18px; line-height: 1.5; margin: 0 0 40px; max-width: 620px; }

.acc-nav { display: flex; align-items: center; justify-content: space-between; padding: 26px 0; }
.acc-links { display: flex; align-items: center; gap: 28px; }
.acc-links a:not(.acc-btn) { text-decoration: none; font-weight: 600; font-size: 15px; }

.acc-hero { display: grid; grid-template-columns: 1.05fr 1fr; gap: 56px; align-items: center; padding: 10px 0 90px; }
.acc-hero > *, .acc-app > *, .acc-host-grid > * { min-width: 0; }
.acc-slate { position: relative; margin: 0; aspect-ratio: 1; container-type: inline-size; transform: rotate(-2deg); filter: drop-shadow(0 28px 30px rgba(40,30,10,.35)) drop-shadow(0 4px 6px rgba(40,30,10,.3)); }
.acc-slate img { position: absolute; inset: 0; width: 100%; height: 100%; }
.acc-chalk { position: absolute; left: 15.5%; right: 15.5%; top: 15%; bottom: 22%; text-align: left; color: #F1EEE3; font-family: 'Caveat', cursive; filter: url(#acc-chalk); text-shadow: 0 0 1px rgba(241,238,227,.35); display: flex; flex-direction: column; }
.acc-who { font-size: 3.4cqw; opacity: .72; }
.acc-title { font-size: 7.8cqw; font-weight: 700; line-height: 1; margin: .3cqw 0 3cqw; }
.acc-menu { display: grid; grid-template-columns: 10.5cqw 1fr; row-gap: 1.2cqw; font-size: 4.25cqw; line-height: 1.12; }
.acc-menu b { color: #D3EC6A; font-weight: 500; opacity: .92; }
.acc-menu small { font-size: 3cqw; opacity: .6; }
.acc-rule { border-top: 2px dashed rgba(241,238,227,.3); margin: auto 0 1.7cqw; }
.acc-foot { display: flex; justify-content: space-between; font-size: 3.4cqw; opacity: .78; }
@container (max-width: 480px) {
  .acc-title { font-size: 9cqw; margin-bottom: 4cqw; }
  .acc-menu { font-size: 5.6cqw; grid-template-columns: 13cqw 1fr; row-gap: 1.8cqw; }
  .acc-menu small, .acc-menu .acc-extra, .acc-rule, .acc-foot { display: none; }
}
.acc-pitch h1 { font-family: var(--title-font), sans-serif; font-weight: 900; font-size: 54px; line-height: 1.02; margin: 0 0 22px; letter-spacing: -.5px; }
.acc-pitch p { font-size: 19px; line-height: 1.55; color: var(--muted); margin: 0 0 30px; max-width: 500px; }
.acc-ctas { display: flex; gap: 12px; flex-wrap: wrap; }
.acc-pitch .acc-note { margin: 16px 0 0; font-size: 14.5px; }

.acc-how { background: var(--card); padding: 90px 0; }
.acc-days { display: grid; grid-template-columns: repeat(4, 1fr); gap: 20px; }
.acc-day { border-radius: 22px; padding: 24px; background: var(--bg); display: flex; flex-direction: column; }
.acc-when { font-size: 30px; color: var(--link); line-height: 1; }
.acc-day h3 { font-size: 18px; margin: 12px 0 6px; }
.acc-day p { margin: 0 0 16px; font-size: 15px; line-height: 1.5; color: var(--muted); }
.acc-mini { margin-top: auto; background: var(--card); border-radius: 14px; padding: 10px 12px; font-size: 13.5px; line-height: 1.4; }

.acc-app { padding: 100px 0; display: grid; grid-template-columns: 1fr 1.1fr; gap: 60px; align-items: center; }
.acc-app .acc-lead { margin-bottom: 30px; }
.acc-points { display: grid; gap: 22px; }
.acc-point { display: grid; grid-template-columns: 44px 1fr; gap: 14px; align-items: start; }
.acc-ic { width: 44px; height: 44px; border-radius: 14px; background: var(--chip); display: grid; place-items: center; color: var(--text); }
.acc-point h3 { margin: 2px 0 4px; font-size: 17px; }
.acc-point p { margin: 0; font-size: 15px; line-height: 1.5; color: var(--muted); }

/* Les téléphones : une illustration de l'appli en Chartreuse jour, quel que soit le thème. */
.acc-phones { position: relative; height: 600px; color: #1C2208; font-family: 'Atkinson Hyperlegible Next', sans-serif; }
.acc-phone { position: absolute; width: 290px; height: 590px; border-radius: 42px; background: #1C2208; padding: 10px; box-shadow: 0 40px 60px -30px rgba(28,34,8,.6); }
.acc-phone-1 { left: -10px; top: 0; transform: rotate(-4deg); }
.acc-phone-2 { left: 300px; top: 50px; transform: rotate(3deg); }
.acc-screen { width: 100%; height: 100%; border-radius: 33px; background: #F2F5E4; overflow: hidden; padding: 18px 14px; position: relative; }
.acc-scr-top { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
.acc-logo-xs { font-family: 'Unbounded', sans-serif; font-weight: 700; font-size: 14px; }
.acc-ava { width: 26px; height: 26px; border-radius: 50%; background: #283210; color: #D7F55A; font-size: 10px; font-weight: 800; display: grid; place-items: center; }
.acc-scr-title { font-family: 'Unbounded', sans-serif; font-weight: 700; font-size: 18px; margin-bottom: 8px; }
.acc-strip { display: flex; gap: 2px; margin-bottom: 12px; }
.acc-strip div { flex: 1; min-width: 0; text-align: center; background: #FFFFFB; border-radius: 9px; padding: 4px 0; font-size: 8px; color: #4F5833; }
.acc-strip div b { display: block; font-size: 10.5px; color: #1C2208; }
.acc-strip .on { background: #C3E534; border: 1.2px solid #1C2208; }
.acc-tonight { background: #C3E534; border: 1.5px solid #1C2208; border-radius: 18px; padding: 12px; }
.acc-vg { width: 52px; height: 52px; border-radius: 13px; background: #C7DFEA; display: grid; place-items: center; margin: 8px 0; }
.acc-scr-dish { font-family: 'Unbounded', sans-serif; font-weight: 700; font-size: 17px; line-height: 1.1; }
.acc-xs { font-size: 10.5px; color: #4F5833; margin: 3px 0; }
.acc-xs-strong { font-size: 10.5px; font-weight: 800; }
.acc-xs-mid { color: #1C2208; font-weight: 600; margin-top: 6px; }
.acc-xxs { font-size: 9.5px; color: #4F5833; }
.acc-warn { background: #fff; border-radius: 10px; padding: 6px 8px; margin-top: 8px; font-size: 10px; color: #875200; font-weight: 600; }
.acc-scr-btns { display: flex; gap: 6px; margin-top: 9px; }
.acc-scr-btns span { border: 1.2px solid #1C2208; border-radius: 10px; padding: 6px 10px; font-size: 10px; font-weight: 600; }
.acc-scr-btns .dark { background: #1C2208; color: #FFFFFB; }
.acc-progress { height: 7px; border-radius: 4px; background: #E9EFCF; overflow: hidden; margin: 8px 0 2px; }
.acc-progress div { width: 42%; height: 100%; background: #C3E534; border: 1px solid #1C2208; border-radius: 4px; }
.acc-list { background: #FFFFFB; border-radius: 16px; padding: 10px 12px; margin-top: 10px; }
.acc-scr-aisle { font-family: 'Unbounded', sans-serif; font-weight: 700; font-size: 13px; margin-bottom: 2px; }
.acc-row { display: flex; align-items: center; gap: 9px; padding: 8px 0; border-bottom: 1px solid #DFE6C3; font-size: 12px; }
.acc-row.last { border: none; }
.acc-ck { width: 17px; height: 17px; border-radius: 50%; border: 1.6px solid #4F5833; flex: none; }
.acc-ck.on { background: #C3E534; border-color: #1C2208; }
.acc-done { text-decoration: line-through; color: #4F5833; }
.acc-q { margin-left: auto; font-weight: 800; }
.acc-tab { position: absolute; left: 0; right: 0; bottom: 0; height: 52px; background: #FFFFFB; border-top: 1px solid #DFE6C3; display: flex; justify-content: space-around; align-items: center; font-size: 9.5px; font-weight: 600; color: #4F5833; }
.acc-tab .act { background: #C3E534; border-radius: 999px; padding: 4px 9px; color: #1C2208; font-weight: 800; }

.acc-join { padding: 0 0 100px; display: grid; grid-template-columns: 1fr 1fr; gap: 24px; align-items: stretch; }
.acc-card { border-radius: 26px; padding: 34px; background: var(--card); border: 1.5px solid var(--line); display: flex; flex-direction: column; align-items: flex-start; }
.acc-accent { background: var(--action); color: var(--on-action); border: 2px solid var(--action-border); }
.acc-card h3 { font-family: var(--title-font), sans-serif; font-weight: 700; font-size: 24px; margin: 0 0 10px; }
.acc-card p { margin: 0 0 22px; font-size: 16.5px; line-height: 1.55; max-width: 460px; }
.acc-card .acc-hand { font-size: 24px; color: var(--link); margin-bottom: 4px; }
.acc-accent .acc-hand { color: inherit; opacity: .75; }
.acc-card .acc-small-print { font-size: 14px; opacity: .8; margin: -8px 0 18px; }
.acc-card .acc-btn + .acc-small-print { margin: 14px 0 0; }
.acc-field { display: grid; gap: 6px; width: 100%; max-width: 460px; margin-bottom: 14px; font-size: 14px; font-weight: 600; }
.acc-field input { font: 16px var(--text-font), sans-serif; padding: 13px 14px; border-radius: 14px; border: 1.5px solid var(--line); background: var(--bg); color: var(--text); width: 100%; }
.acc-error { color: #B23A1E; font-weight: 600; font-size: 14.5px !important; margin: -4px 0 14px !important; }

.acc-host { background: #11140A; color: #EEF3DC; padding: 84px 0; }
.acc-host-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 56px; align-items: center; }
.acc-host .acc-hand { color: #C9F23F; font-size: 26px; opacity: .9; }
.acc-host h2 { font-family: 'Unbounded', sans-serif; font-weight: 700; font-size: 34px; margin: 4px 0 14px; }
.acc-host p { color: #AEB78F; font-size: 17px; line-height: 1.55; margin: 0 0 26px; max-width: 470px; }
.acc-facts { display: flex; gap: 34px; flex-wrap: wrap; }
.acc-facts b { display: block; font-family: 'Unbounded', sans-serif; font-size: 26px; color: #C9F23F; }
.acc-facts span { font-size: 13.5px; color: #AEB78F; }
.acc-term { background: #1C2111; border: 1px solid #2F361A; border-radius: 18px; padding: 22px 24px; color: #EEF3DC; }
.acc-term code { display: block; font: 13px/1.95 ui-monospace, Consolas, monospace; overflow-wrap: anywhere; }
.acc-term a { display: inline-block; margin-top: 14px; color: #C9F23F; font-weight: 600; text-decoration: none; }
.acc-term :focus-visible { outline-color: #C9F23F; }
.acc-p { color: #C9F23F; }
.acc-c { color: #7f8866; }
.acc-dots { display: flex; gap: 6px; margin-bottom: 12px; }
.acc-dots i { width: 10px; height: 10px; border-radius: 50%; background: #2F361A; display: block; }
.acc-footer { background: #11140A; border-top: 1px solid #2F361A; color: #7f8866; font-size: 13.5px; padding: 24px 0; }
.acc-footer .acc-wrap { display: flex; justify-content: space-between; align-items: center; }
.acc-footer a { color: #AEB78F; text-decoration: none; margin-left: 22px; }

/* Tablette : l'accroche en une colonne, l'ardoise centrée. */
@media (max-width: 1024px) {
  .acc-wrap { padding: 0 32px; }
  .acc-hero { grid-template-columns: 1fr; gap: 36px; padding: 0 0 70px; justify-items: center; text-align: center; }
  .acc-slate { width: min(100%, 560px); }
  .acc-pitch p { margin-left: auto; margin-right: auto; }
  .acc-ctas { justify-content: center; }
  .acc-days { grid-template-columns: 1fr 1fr; }
  .acc-app { grid-template-columns: 1fr; gap: 40px; padding: 70px 0; }
  .acc-phones { width: 560px; max-width: 100%; margin: 0 auto; }
  .acc-host-grid { grid-template-columns: 1fr; gap: 32px; }
}
/* Téléphone */
@media (max-width: 640px) {
  .acc-wrap { padding: 0 16px; }
  .acc-nav { padding: 16px 0; }
  .acc-links a:not(.acc-btn) { display: none; }
  .acc-hero { gap: 28px; padding: 0 0 56px; text-align: left; justify-items: stretch; }
  .acc-slate { width: calc(100% + 12px); margin: 0 -6px; transform: rotate(-1deg); }
  .acc-pitch h1 { font-size: 34px; margin-bottom: 14px; }
  .acc-pitch p { font-size: 17px; margin-bottom: 22px; }
  .acc-ctas { justify-content: stretch; }
  .acc-ctas .acc-btn { flex: 1 1 auto; text-align: center; }
  .acc-h2 { font-size: 25px; }
  .acc-lead { font-size: 16px; margin-bottom: 24px; }
  .acc-how { padding: 56px 0; }
  /* Les quatre moments en carrousel : on glisse de l'un à l'autre. */
  .acc-days { display: flex; overflow-x: auto; scroll-snap-type: x mandatory; gap: 12px; margin: 0 -16px; padding: 0 16px 6px; scrollbar-width: none; }
  .acc-day { flex: 0 0 82%; scroll-snap-align: start; }
  .acc-app { padding: 56px 0; gap: 36px; }
  .acc-phones { height: 560px; width: 100%; }
  .acc-phone-1 { left: 50%; transform: translateX(-50%) rotate(-2deg); }
  .acc-phone-2 { display: none; }
  .acc-join { grid-template-columns: 1fr; padding-bottom: 56px; gap: 14px; }
  .acc-card { padding: 24px; }
  .acc-card h3 { font-size: 21px; }
  .acc-host { padding: 56px 0; }
  .acc-host h2 { font-size: 26px; }
  .acc-facts { gap: 20px; }
  .acc-facts b { font-size: 21px; }
  .acc-term { padding: 16px; }
  .acc-term code { font-size: 12px; }
  .acc-footer .acc-wrap { flex-direction: column; gap: 10px; align-items: flex-start; }
  .acc-footer a { margin: 0 0 0 16px; }
}
@media (prefers-reduced-motion: reduce) { .acc { scroll-behavior: auto; } }
`;
