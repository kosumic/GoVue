import assert from 'node:assert/strict';
import { generateKeyPairSync } from 'node:crypto';
import { createRequire } from 'node:module';
import test from 'node:test';

const require = createRequire(import.meta.url);
const braces = require('braces');
const forge = require('node-forge');

test('brace parsing and traversal reject excessive nesting before overflowing the stack', () => {
  for (const [open, close] of [['{', '}'], ['(', ')']]) {
    const pattern = open.repeat(4000) + 'x' + close.repeat(4000);
    for (const operation of ['parse', 'compile', 'expand', 'stringify']) {
      assert.throws(() => braces[operation](pattern), SyntaxError);
    }
  }
  let ast = { type: 'text', value: 'x' };
  for (let i = 0; i < 12000; i++) ast = { type: 'root', nodes: [ast] };
  const cycle = { type: 'root', nodes: [] };
  cycle.nodes.push(cycle);
  for (const operation of ['compile', 'expand', 'stringify']) {
    assert.throws(() => braces[operation](ast), SyntaxError);
    assert.throws(() => braces[operation](cycle), SyntaxError);
  }
});

test('brace guards preserve ranges, normal nesting, escaping and shared acyclic nodes', () => {
  assert.deepEqual(braces.expand('a/{1..3}/{x,y}'), [
    'a/1/x', 'a/1/y', 'a/2/x', 'a/2/y', 'a/3/x', 'a/3/y',
  ]);
  assert.doesNotThrow(() => braces.compile('{'.repeat(64) + 'x,y' + '}'.repeat(64)));
  assert.equal(braces.stringify('\\{literal\\}'), '{literal}');
  const leaf = { type: 'text', value: 'a' };
  assert.equal(braces.stringify({ type: 'root', nodes: [leaf, leaf] }), 'aa');
});

test('RSA verification rejects extra nested DigestAlgorithm elements and accepts valid signatures', () => {
  const { privateKey } = generateKeyPairSync('rsa', {
    modulusLength: 1024,
    privateKeyEncoding: { type: 'pkcs1', format: 'pem' },
    publicKeyEncoding: { type: 'pkcs1', format: 'pem' },
  });
  const key = forge.pki.privateKeyFromPem(privateKey);
  const pub = forge.pki.rsa.setPublicKey(key.n, key.e);
  const md = forge.md.sha256.create().update('security regression');
  const digest = md.digest().getBytes();
  assert.equal(pub.verify(digest, key.sign(md)), true);
  assert.equal(pub.verify('x'.repeat(32), key.sign(md)), false);

  const a = forge.asn1;
  const node = (type, value, constructed = false) => a.create(a.Class.UNIVERSAL, type, constructed, value);
  for (const withNull of [false, true]) {
    const children = [node(a.Type.OID, a.oidToDer(forge.oids.sha256).getBytes())];
    if (withNull) children.push(node(a.Type.NULL, ''));
    const algorithm = node(a.Type.SEQUENCE, children, true);
    const info = node(a.Type.SEQUENCE, [algorithm, node(a.Type.OCTETSTRING, digest)], true);
    const sign = () => key.sign(a.toDer(info).getBytes(), 'NONE');
    assert.equal(pub.verify(digest, sign()), true);

    // The outer DigestInfo is still valid: the surplus element is nested in
    // its algorithm sequence, bypassing the previous outer-only count check.
    algorithm.value.push(node(a.Type.OCTETSTRING, 'unconsumed data'));
    assert.throws(() => pub.verify(digest, sign()), /valid RSASSA-PKCS1-v1_5 DigestInfo/);
  }
});
