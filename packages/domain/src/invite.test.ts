import assert from 'node:assert/strict';
import { test } from 'node:test';

import { inviteToken } from './invite.ts';

test('jeton d’un lien d’invitation collé', () => {
  assert.equal(inviteToken('https://chaudron.example.org/invitation/Ab3_x-9QzK'), 'Ab3_x-9QzK');
  assert.equal(inviteToken('  chaudron.example.org/invitation/Ab3_x-9QzK?ref=sms \n'), 'Ab3_x-9QzK');
  assert.equal(inviteToken('Ab3_x-9QzK'), 'Ab3_x-9QzK');
  assert.equal(inviteToken(''), null);
  assert.equal(inviteToken('bonjour'), null);
  assert.equal(inviteToken('https://chaudron.example.org/recettes'), null);
});
