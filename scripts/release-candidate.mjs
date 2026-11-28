import { createHash } from 'node:crypto';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export const candidateFilename = 'dezirclip-candidate.json';
export const candidateSealJob = 'Seal verified release candidate';
const maximumBytes = 64 * 1024;
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function candidateBytes({ tag, source, releaseId, repository, run, products, inputs, channel, manifest }) {
  const { workflow_id, run_id, run_attempt, product_cutoffs } = run;
  const files = JSON.parse(products);
  if (files.length !== 40) throw new Error('Sealed release policy requires 40 product assets');
  const coverage = JSON.stringify(manifest, null, 2) + '\n';
  return Buffer.from(JSON.stringify({
    schema_version: 1,
    policy: 'dual-desktop-arch-v1',
    repository,
    tag,
    source_revision: source,
    release_id: releaseId,
    original_run: { workflow_id, run_id, run_attempt },
    product_cutoffs: Object.fromEntries(Object.entries(product_cutoffs)
      .map(([job, cutoff]) => [job, new Date(cutoff).toISOString()])),
    products: files,
    inputs,
    channel: { name: channel, asset_name: channel + '.json', sha256: sha256(coverage) },
  }, null, 2) + '\n');
}

export function requireCandidateAsset(release, bytes, interval, download, required = true) {
  const matches = release.assets.filter(asset => asset.name === candidateFilename);
  if (matches.length === 0 && !required) return null;
  if (matches.length !== 1 || bytes.length === 0 || bytes.length > maximumBytes) {
    throw new Error('Invalid or missing frozen candidate attachment');
  }
  const [asset] = matches;
  const created = Date.parse(asset.created_at), updated = Date.parse(asset.updated_at);
  const start = Date.parse(interval.started_at);
  const end = interval.completed_at == null ? Infinity : Date.parse(interval.completed_at);
  if (!Number.isSafeInteger(asset.id) || asset.id <= 0 || release.assets.some(other => other !== asset && other.id === asset.id)
      || asset.state !== 'uploaded' || asset.size !== bytes.length || asset.digest !== 'sha256:' + sha256(bytes)
      || !Number.isFinite(start) || !(created >= start && updated >= created && updated <= end)
      || !bytes.equals(download(asset))) {
    throw new Error('Frozen candidate content, asset identity or seal time differs');
  }
  return { id: asset.id, name: asset.name, size: asset.size, sha256: sha256(bytes),
    created_at: asset.created_at, updated_at: asset.updated_at };
}

export function attachCandidate({ tag, repository, bytes, interval, readDraft, download, transport }) {
  const existing = requireCandidateAsset(readDraft(), bytes, interval, download, false);
  if (existing) return existing;
  if (bytes.length > maximumBytes) throw new Error('Frozen candidate exceeds its size limit');
  const directory = mkdtempSync(join(tmpdir(), 'dezirclip-candidate-'));
  try {
    const path = join(directory, candidateFilename);
    writeFileSync(path, bytes, { flag: 'wx' });
    try {
      transport(['release', 'upload', tag, '--repo', repository, path]);
    } catch (error) {
      // A successful upload can lose its response. Reuse the exact remote bytes.
      const committed = requireCandidateAsset(readDraft(), bytes, interval, download, false);
      if (committed) return committed;
      throw error;
    }
    return requireCandidateAsset(readDraft(), bytes, interval, download);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}
