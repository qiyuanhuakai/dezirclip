import assert from 'node:assert/strict';
import test from 'node:test';
import { verifySurfaceIdentity } from '../build-surface.mjs';
import { MYGO_PATCH_ID } from '../mygo-surface-patch.mjs';

const expected = {
  component_version: '0.2.0-alpha.5', source_revision: 'a'.repeat(40), source_state: 'verified_checkout',
  build_id: `0.2.0-alpha.5@${'a'.repeat(40)}`, target: 'x86_64-pc-windows-msvc',
  toolkit_patch_id: MYGO_PATCH_ID,
};
const identity = () => ({ ...expected, component: 'surface', mygo_version: 'v0.6.4', protocol_version: { major: 1, minor: 0 } });

test('matching native helper metadata is accepted without substituting desktop metadata', () => {
  verifySurfaceIdentity(identity(), expected);
});

test('another source, version, platform, or modified helper cannot pass as the expected clean build', () => {
  for (const [field, value] of Object.entries({
    component_version: '0.2.0-alpha.4', source_revision: 'b'.repeat(40), source_state: 'modified_checkout',
    build_id: `0.2.0-alpha.5@${'b'.repeat(40)}`, target: 'aarch64-pc-windows-msvc', toolkit_patch_id: 'unknown',
  })) assert.throws(() => verifySurfaceIdentity({ ...identity(), [field]: value }, expected), new RegExp(field));
});

test('a different component, MyGo pin, or protocol fails even with matching application identity', () => {
  for (const change of [{ component: 'desktop' }, { mygo_version: 'v0.6.3' },
    { protocol_version: { major: '1', minor: 0 } }, { protocol_version: { major: 1, minor: 1 } },
    { protocol_version: null }]) {
    assert.throws(() => verifySurfaceIdentity({ ...identity(), ...change }, expected), /incompatible/);
  }
});
