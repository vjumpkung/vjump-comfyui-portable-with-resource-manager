import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

import { check } from './check-arch-deps.mjs';

function fixture(name) {
  return JSON.parse(readFileSync(new URL(`./fixtures/arch/${name}.json`, import.meta.url), 'utf8'));
}

test('clean workspace allows inward edges and only checks direct dependencies', () => {
  assert.deepEqual(check(fixture('clean')), []);
});

for (const [name, violation] of [
  ['renamed-dep', /domain: forbidden direct dependency "serde"/],
  ['optional-dep', /domain: forbidden direct dependency "serde"/],
  ['build-dep', /application: forbidden direct dependency "tokio"/],
  ['dev-dep', /application: forbidden direct dependency "serde"/],
  ['target-specific-dep', /domain: forbidden direct dependency "tokio"/],
  ['forbidden-workspace-edge', /application: forbidden workspace edge to "infrastructure"/],
  ['tauri-in-infrastructure', /infrastructure: forbidden direct dependency "tauri-plugin-log"/],
]) {
  test(`rejects ${name}`, () => {
    assert.ok(check(fixture(name)).some((message) => violation.test(message)));
  });
}

test('resolved identity remains authoritative when a declared alias looks allowed', () => {
  const metadata = fixture('renamed-dep');
  metadata.packages.find((pkg) => pkg.name === 'domain').dependencies = [];
  assert.ok(check(metadata).some((message) => message.includes('"serde"')));
});

test('checker does not mutate its metadata input', () => {
  const metadata = fixture('clean');
  const before = structuredClone(metadata);
  check(metadata);
  assert.deepEqual(metadata, before);
});
