import { execFileSync } from 'node:child_process';
import { appendFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { repository } from './updater-release.mjs';

const gateJob = 'Verify actual signed Windows upgrades / Bind all actual final installer upgrades';
const positive = value => Number.isSafeInteger(value) && value > 0;

export function selectUpgradeArtifact(run, jobs, artifacts, runId, attempt, tag) {
  if (!positive(runId) || !positive(attempt) || run?.id !== runId || run.run_attempt !== attempt
      || run.event !== 'push' || run.path !== '.github/workflows/release.yml' || run.head_branch !== tag
      || run.repository?.full_name !== repository || run.head_repository?.full_name !== repository
      || !/^[a-f0-9]{40}$/.test(run.head_sha ?? '')) throw new Error('Original release identity differs');
  const matches = jobs.filter(job => job.name === gateJob);
  const job = matches[0];
  if (matches.length !== 1 || !positive(job.id) || job.run_id !== runId || job.run_attempt !== attempt
      || job.head_sha !== run.head_sha || job.status !== 'completed' || job.conclusion !== 'success') {
    throw new Error('Original upgrade aggregate job must have succeeded');
  }
  const start = Date.parse(job.started_at), end = Date.parse(job.completed_at);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) throw new Error('Invalid gate job interval');
  // GitHub retains successful jobs on a partial rerun, but their artifact names keep the original attempt.
  const candidates = artifacts.filter(artifact => {
    const match = /^release-windows-upgrade-gate-attempt-([1-9]\d*)$/.exec(artifact.name ?? '');
    const created = Date.parse(artifact.created_at);
    return match && positive(Number(match[1])) && Number(match[1]) <= attempt
      && created >= start && created <= end;
  });
  const artifact = candidates[0];
  if (candidates.length !== 1 || !positive(artifact.id) || artifact.expired !== false
      || !positive(artifact.size_in_bytes) || artifact.workflow_run?.id !== runId
      || artifact.workflow_run?.head_sha !== run.head_sha
      || artifact.workflow_run?.head_branch !== tag) {
    throw new Error('Require one unexpired upgrade artifact from the successful aggregate job');
  }
  return { artifact_id: artifact.id, artifact_name: artifact.name, run_id: runId,
    run_attempt: attempt, job_id: job.id, source_revision: run.head_sha };
}

export function completePages(pages, key) {
  const count = pages?.[0]?.total_count;
  if (!Array.isArray(pages) || !pages.length || !Number.isSafeInteger(count) || count < 0
      || pages.some(page => page.total_count !== count || !Array.isArray(page[key]))) {
    throw new Error('Incomplete GitHub listing');
  }
  const values = pages.flatMap(page => page[key]);
  if (values.length !== count || new Set(values.map(value => value.id)).size !== count) {
    throw new Error('Incomplete or duplicate GitHub listing');
  }
  return values;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [runId, attempt, tag, output] = process.argv.slice(2);
  if (!/^[1-9]\d*$/.test(runId ?? '') || !/^[1-9]\d*$/.test(attempt ?? '') || !tag || !output) {
    throw new Error('Usage: release-upgrade-artifact.mjs RUN_ID ATTEMPT TAG RECEIPT.json');
  }
  const api = (route, paginate = false) => JSON.parse(execFileSync('gh', ['api',
    `repos/${repository}/${route}`, ...(paginate ? ['--paginate', '--slurp'] : [])],
  { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }));
  const run = api(`actions/runs/${runId}`);
  const jobs = completePages(api(`actions/runs/${runId}/attempts/${attempt}/jobs?per_page=100`, true), 'jobs');
  const artifacts = completePages(api(`actions/runs/${runId}/artifacts?per_page=100`, true), 'artifacts');
  const selected = selectUpgradeArtifact(run, jobs, artifacts, Number(runId), Number(attempt), tag);
  writeFileSync(output, JSON.stringify(selected, null, 2) + '\n');
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `artifact_id=${selected.artifact_id}\n`);
  console.log(JSON.stringify(selected));
}
