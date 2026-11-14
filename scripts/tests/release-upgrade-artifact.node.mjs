import test from 'node:test';
import assert from 'node:assert/strict';
import { completePages, selectUpgradeArtifact } from '../release-upgrade-artifact.mjs';

function fixture() {
  const run = { id: 17, run_attempt: 2, event: 'push', path: '.github/workflows/release.yml',
    head_branch: 'v0.2.0-beta.2', head_sha: 'a'.repeat(40),
    repository: { full_name: 'qiyuanhuakai/dezirclip' }, head_repository: { full_name: 'qiyuanhuakai/dezirclip' } };
  const job = { id: 30, run_id: 17, run_attempt: 2, head_sha: run.head_sha,
    name: 'Verify actual signed Windows upgrades / Bind all actual final installer upgrades',
    status: 'completed', conclusion: 'success', started_at: '2026-11-14T01:00:00Z', completed_at: '2026-11-14T01:01:00Z' };
  const artifact = { id: 40, name: 'release-windows-upgrade-gate-attempt-1', expired: false, size_in_bytes: 100,
    created_at: '2026-11-14T01:00:45Z', workflow_run: { id: 17, head_sha: run.head_sha, head_branch: run.head_branch } };
  return { run, jobs: [job], artifacts: [artifact] };
}
const select = f => selectUpgradeArtifact(f.run, f.jobs, f.artifacts, 17, 2, 'v0.2.0-beta.2');

test('a retained successful job resolves its original artifact after a partial rerun', () => {
  assert.equal(select(fixture()).artifact_id, 40);
});
test('a rerun aggregate resolves its new artifact rather than an older success', () => {
  const f = fixture();
  f.jobs[0].started_at = '2026-11-14T02:00:00Z'; f.jobs[0].completed_at = '2026-11-14T02:01:00Z';
  f.artifacts.push({ ...f.artifacts[0], id: 41, name: 'release-windows-upgrade-gate-attempt-2', created_at: '2026-11-14T02:00:45Z' });
  assert.equal(select(f).artifact_id, 41);
});
for (const [name, mutate] of [
  ['missing', f => { f.artifacts = []; }],
  ['expired', f => { f.artifacts[0].expired = true; }],
  ['duplicate', f => { f.artifacts.push({ ...f.artifacts[0], id: 41 }); }],
  ['different run', f => { f.artifacts[0].workflow_run.id++; }],
  ['different source', f => { f.artifacts[0].workflow_run.head_sha = 'b'.repeat(40); }],
  ['different tag', f => { f.artifacts[0].workflow_run.head_branch = 'v0.2.0-beta.1'; }],
  ['future attempt', f => { f.artifacts[0].name = 'release-windows-upgrade-gate-attempt-3'; }],
  ['failed gate', f => { f.jobs[0].conclusion = 'failure'; }],
  ['missing gate', f => { f.jobs = []; }],
  ['new gate without new artifact', f => { f.jobs[0].started_at = '2026-11-14T01:00:50Z'; }],
  ['artifact after job', f => { f.artifacts[0].created_at = '2026-11-14T02:00:00Z'; }],
  ['invalid interval', f => { f.jobs[0].started_at = 'invalid'; }],
  ['stale attempt', f => { f.run.run_attempt++; }],
  ['different event', f => { f.run.event = 'workflow_dispatch'; }],
  ['different repository', f => { f.run.repository.full_name = 'elsewhere/repo'; }],
]) test(`reject ${name}`, () => { const f = fixture(); mutate(f); assert.throws(() => select(f)); });

test('paginated listings require complete distinct records', () => {
  assert.deepEqual(completePages([{ total_count: 2, jobs: [{ id: 1 }] }, { total_count: 2, jobs: [{ id: 2 }] }], 'jobs'), [{ id: 1 }, { id: 2 }]);
  for (const pages of [[], [{ total_count: 2, jobs: [{ id: 1 }] }],
    [{ total_count: 2, jobs: [{ id: 1 }, { id: 1 }] }], [{ total_count: 1, jobs: [] }, { total_count: 0, jobs: [] }]]) {
    assert.throws(() => completePages(pages, 'jobs'));
  }
});
