-- M3 : fraîcheur. Combien de jours un ingrédient frais tient au réfrigérateur : par défaut selon son rayon (poisson 2,
-- boucherie et boulangerie 3, fruits et légumes, crèmerie et frais 7) ; `keeps_days` le précise quand il diffère.
ALTER TABLE ingredients ADD COLUMN keeps_days smallint CHECK (keeps_days BETWEEN 1 AND 60);
UPDATE ingredients SET keeps_days = 1 WHERE code = 'minced_beef';
UPDATE ingredients SET keeps_days = 3 WHERE code = 'spinach';
UPDATE ingredients SET keeps_days = 4 WHERE code = 'mushroom';

-- Ingrédients d'un plat congelés en rentrant des courses (codes) : rappel la veille de les sortir.
ALTER TABLE menu_slots ADD COLUMN thaw text[] NOT NULL DEFAULT '{}';
