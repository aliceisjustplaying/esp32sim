#!/usr/bin/env node
// Preserve local experiment bytes and Git states without modifying their sources.
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { pipeline } from 'node:stream/promises';
import { Transform } from 'node:stream';
import { createInterface } from 'node:readline';

const [command, archive, configFile] = process.argv.slice(2);
if (!['capture', 'verify', 'restore-file', 'restore-tree'].includes(command) || !path.isAbsolute(archive ?? '')) {
  throw new Error('Usage: preserve-experiments.mjs capture|verify ARCHIVE CONFIG; restore-file ARCHIVE OBJECT_HASH DESTINATION; restore-tree ARCHIVE ROOT_ID RELATIVE_PREFIX DESTINATION');
}
const sha = () => crypto.createHash('sha256');
const digest = async file => { const h = sha(); for await (const b of fs.createReadStream(file)) h.update(b); return h.digest('hex'); };
const json = (file, value) => fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n');
const git = (cwd, args) => execFileSync('git', ['-C', cwd, ...args], { stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 128 * 1024 * 1024 });
const objectPath = hash => path.join(archive, 'objects', hash.slice(0, 2), hash);
const inside = (file, parent) => file === parent || file.startsWith(parent + path.sep);
const progress = value => { json(path.join(archive, 'progress.json'), { at: new Date().toISOString(), ...value }); console.log(JSON.stringify(value)); };

async function checkedCopy(source, destination, expected, mode = 0o444) {
  const hash = sha();
  await pipeline(fs.createReadStream(source), new Transform({ transform(b, _, cb) { hash.update(b); cb(null, b); } }), fs.createWriteStream(destination, { flags: 'wx', mode: 0o600 }));
  if (hash.digest('hex') !== expected) throw new Error('Copy hash mismatch: ' + source);
  await fsp.chmod(destination, mode);
}

async function capture() {
  const config = JSON.parse(fs.readFileSync(configFile));
  if (fs.existsSync(path.join(archive, 'COMPLETE.json'))) throw new Error('Archive already complete');
  fs.mkdirSync(archive, { recursive: true, mode: 0o700 });
  fs.chmodSync(archive, 0o700);
  json(path.join(archive, 'config.json'), config);
  const records = [], exclusions = [], gitRoots = new Set();
  const exclude = [archive, ...config.exclude.map(x => x.path)];
  function walk(root, directory, relative = '') {
    for (const name of fs.readdirSync(directory).sort()) {
      const source = path.join(directory, name), rel = path.join(relative, name);
      if (exclude.some(x => inside(source, x))) { exclusions.push({ path: source, reason: config.exclude.find(x => inside(source, x.path))?.reason ?? 'archive destination' }); continue; }
      if (name === '.git') {
        let repository = false;
        try { git(directory, ['rev-parse', '--git-common-dir']); repository = true; } catch { /* Some dependency caches use an empty .git sentinel. */ }
        if (repository) { gitRoots.add(directory); exclusions.push({ path: source, reason: 'Git state preserved separately in bundles and worktree records' }); continue; }
      }
      const st = fs.lstatSync(source, { bigint: true });
      const item = { root: root.id, path: rel, mode: Number(st.mode & 0o777n), mtimeNs: String(st.mtimeNs) };
      if (st.isDirectory()) { records.push({ ...item, type: 'directory' }); walk(root, source, rel); }
      else if (st.isFile()) records.push({ ...item, type: 'file', bytes: Number(st.size), dev: String(st.dev), ino: String(st.ino) });
      else if (st.isSymbolicLink()) records.push({ ...item, type: 'symlink', target: fs.readlinkSync(source) });
      else exclusions.push({ path: source, reason: 'ephemeral special filesystem object', mode: Number(st.mode) });
    }
  }
  for (const root of config.roots) walk(root, root.path);
  json(path.join(archive, 'exclusions.json'), exclusions);
  progress({ phase: 'scanned', records: records.length, files: records.filter(x => x.type === 'file').length });

  // Every distinct repository gets a self-contained bundle, including detached worktree heads.
  const groups = new Map(), worktrees = [];
  for (const cwd of gitRoots) {
    const common = git(cwd, ['rev-parse', '--path-format=absolute', '--git-common-dir']).toString().trim();
    if (!groups.has(common)) groups.set(common, []);
    groups.get(common).push(cwd);
  }
  fs.mkdirSync(path.join(archive, 'git'), { recursive: true });
  let repoNumber = 0;
  for (const [common, checkouts] of groups) {
    const id = 'repo-' + String(++repoNumber).padStart(2, '0'), staging = path.join(archive, 'git', id + '-staging.git');
    const bundle = path.join(archive, 'git', id + '.bundle');
    if (fs.existsSync(staging)) throw new Error('Staging repository already exists: ' + staging);
    execFileSync('git', ['clone', '--mirror', '--shared', common, staging], { stdio: ['ignore', 'pipe', 'pipe'] });
    const refs = git(checkouts[0], ['for-each-ref', '--format=%(refname) %(objectname)']).toString();
    fs.writeFileSync(path.join(archive, 'git', id + '-original-refs.txt'), refs);
    json(path.join(archive, 'git', id + '-origin.json'), { common, checkouts });
    for (let n = 0; n < checkouts.length; n++) {
      const cwd = checkouts[n], key = id + '-worktree-' + String(n + 1).padStart(3, '0');
      const head = git(cwd, ['rev-parse', 'HEAD']).toString().trim();
      git(staging, ['update-ref', 'refs/preservation/' + key, head]);
      const status = git(cwd, ['status', '--porcelain=v1', '-z', '--untracked-files=all']);
      fs.writeFileSync(path.join(archive, 'git', key + '-status.z'), status);
      fs.writeFileSync(path.join(archive, 'git', key + '-staged.patch'), git(cwd, ['diff', '--cached', '--binary', '--full-index']));
      fs.writeFileSync(path.join(archive, 'git', key + '-unstaged.patch'), git(cwd, ['diff', '--binary', '--full-index']));
      worktrees.push({ key, source: cwd, head, branch: git(cwd, ['branch', '--show-current']).toString().trim() || null, bundle: 'git/' + id + '.bundle', statusBytes: status.length });
    }
    git(staging, ['bundle', 'create', bundle, '--all']);
    fs.writeFileSync(path.join(archive, 'git', id + '-bundle-verify.txt'), git(staging, ['bundle', 'verify', bundle]));
    // This directory is created by this invocation and uses the source only through alternates.
    fs.rmSync(staging, { recursive: true });
  }
  json(path.join(archive, 'worktrees.json'), worktrees);

  const files = records.filter(x => x.type === 'file'), inflight = new Map();
  let cursor = 0, finished = 0, uniqueBytes = 0, uniqueFiles = 0;
  await Promise.all(Array.from({ length: 6 }, async () => {
    while (cursor < files.length) {
      const item = files[cursor++], root = config.roots.find(x => x.id === item.root), source = path.join(root.path, item.path);
      item.sha256 = await digest(source);
      const st = await fsp.lstat(source, { bigint: true });
      if (Number(st.size) !== item.bytes || String(st.mtimeNs) !== item.mtimeNs) throw new Error('Source changed during capture: ' + source);
      if (!inflight.has(item.sha256)) inflight.set(item.sha256, (async () => {
        const destination = objectPath(item.sha256);
        await fsp.mkdir(path.dirname(destination), { recursive: true });
        if (fs.existsSync(destination) && await digest(destination) !== item.sha256) fs.unlinkSync(destination);
        if (!fs.existsSync(destination)) {
          const free = fs.statfsSync(archive);
          if (free.bavail * free.bsize < item.bytes + 3 * 2 ** 30) throw new Error('Preservation stopped with less than 3 GiB reserve');
          await checkedCopy(source, destination, item.sha256);
        }
        uniqueBytes += item.bytes; uniqueFiles++;
      })());
      await inflight.get(item.sha256);
      delete item.dev; delete item.ino;
      if (++finished % 10000 === 0) progress({ phase: 'capturing', finished, total: files.length, uniqueFiles, uniqueBytes });
    }
  }));
  const manifest = fs.createWriteStream(path.join(archive, 'files.jsonl'));
  for (const item of records) if (!manifest.write(JSON.stringify(item) + '\n')) await new Promise(resolve => manifest.once('drain', resolve));
  await new Promise(resolve => manifest.end(resolve));

  // Readable paths share immutable object bytes. Original modes live in the manifest.
  for (const root of config.roots) fs.mkdirSync(path.join(archive, 'files', root.id), { recursive: true });
  const links = [];
  for (const item of records) {
    const dest = path.join(archive, 'files', item.root, item.path);
    if (item.type === 'directory') fs.mkdirSync(dest, { recursive: true });
    if (item.type === 'file') { fs.mkdirSync(path.dirname(dest), { recursive: true }); fs.linkSync(objectPath(item.sha256), dest); }
    if (item.type === 'symlink') {
      const root = config.roots.find(x => x.id === item.root), absolute = path.resolve(path.dirname(path.join(root.path, item.path)), item.target);
      const owner = config.roots.find(x => inside(absolute, x.path));
      const relocated = owner && !exclude.some(x => inside(absolute, x)) ? path.join(archive, 'files', owner.id, path.relative(owner.path, absolute)) : null;
      const target = relocated ? path.relative(path.dirname(dest), relocated) : item.target;
      fs.mkdirSync(path.dirname(dest), { recursive: true }); fs.symlinkSync(target, dest);
      links.push({ root: item.root, path: item.path, originalTarget: item.target, archiveTarget: target, external: !relocated, exists: fs.existsSync(dest) });
    }
  }
  json(path.join(archive, 'symlinks.json'), links);
  const summary = { capturedAt: new Date().toISOString(), roots: config.roots, records: records.length, files: files.length, logicalBytes: files.reduce((s, x) => s + x.bytes, 0), uniqueFiles, uniqueBytes, gitRepositories: groups.size, worktrees: worktrees.length, exclusions: exclusions.length, externalSymlinks: links.filter(x => x.external).length, manifestSha256: await digest(path.join(archive, 'files.jsonl')) };
  json(path.join(archive, 'CAPTURED.json'), summary);
  progress({ phase: 'captured', ...summary });
}

async function verify() {
  const captured = JSON.parse(fs.readFileSync(path.join(archive, 'CAPTURED.json')));
  if (await digest(path.join(archive, 'files.jsonl')) !== captured.manifestSha256) throw new Error('Manifest hash mismatch');
  const fresh = path.join(archive, 'verification-restore');
  fs.mkdirSync(fresh); // Refuse reuse of an old verification directory.
  const seen = new Set(), entries = new Map(); let records = 0, restoredObjects = 0, restoredBytes = 0;
  const links = new Map(JSON.parse(fs.readFileSync(path.join(archive, 'symlinks.json'))).map(x => [x.root + '/' + x.path, x.archiveTarget]));
  for await (const line of createInterface({ input: fs.createReadStream(path.join(archive, 'files.jsonl')) })) {
    const item = JSON.parse(line), view = path.join(archive, 'files', item.root, item.path);
    if (item.type !== 'directory') entries.set(item.root + '/' + item.path, item);
    if (item.type === 'file') {
      const object = objectPath(item.sha256), a = fs.statSync(view), b = fs.statSync(object);
      if (a.ino !== b.ino || a.dev !== b.dev || a.size !== item.bytes) throw new Error('View mismatch: ' + view);
      if (!seen.has(item.sha256)) {
        const restored = path.join(fresh, item.sha256);
        await checkedCopy(object, restored, item.sha256, item.mode);
        if (await digest(restored) !== item.sha256) throw new Error('Restored hash mismatch');
        if ((fs.statSync(restored).mode & 0o777) !== item.mode) throw new Error('Restored mode mismatch');
        fs.unlinkSync(restored); seen.add(item.sha256); restoredObjects++; restoredBytes += item.bytes;
      }
    } else if (item.type === 'directory' && !fs.statSync(view).isDirectory()) throw new Error('Directory mismatch');
    else if (item.type === 'symlink' && (!fs.lstatSync(view).isSymbolicLink() || fs.readlinkSync(view) !== links.get(item.root + '/' + item.path))) throw new Error('Symlink mismatch');
    if (++records % 20000 === 0) progress({ phase: 'verifying', records, restoredObjects, restoredBytes });
  }
  const bundles = fs.readdirSync(path.join(archive, 'git')).filter(x => x.endsWith('.bundle'));
  const worktrees = JSON.parse(fs.readFileSync(path.join(archive, 'worktrees.json')));
  let restoredWorktrees = 0;
  for (const file of bundles) {
    const restored = path.join(fresh, file + '.git');
    execFileSync('git', ['clone', '--mirror', path.join(archive, 'git', file), restored], { stdio: ['ignore', 'pipe', 'pipe'] });
    fs.writeFileSync(path.join(archive, 'git', file + '-restored-fsck.txt'), git(restored, ['fsck', '--full']));
    for (const line of fs.readFileSync(path.join(archive, 'git', file.replace('.bundle', '-original-refs.txt')), 'utf8').trim().split('\n').filter(Boolean)) {
      const [ref, hash] = line.split(' ');
      if (git(restored, ['rev-parse', ref]).toString().trim() !== hash) throw new Error('Restored ref mismatch: ' + ref);
    }
    for (const wt of worktrees.filter(x => x.bundle === 'git/' + file)) {
      const checkout = path.join(fresh, wt.key);
      execFileSync('git', ['init', '--quiet', checkout], { stdio: ['ignore', 'pipe', 'pipe'] });
      fs.mkdirSync(path.join(checkout, '.git/objects/info'), { recursive: true });
      fs.writeFileSync(path.join(checkout, '.git/objects/info/alternates'), path.join(restored, 'objects') + '\n');
      git(checkout, ['update-ref', 'HEAD', wt.head]);
      git(checkout, ['read-tree', wt.head]);
      const staged = path.join(archive, 'git', wt.key + '-staged.patch');
      if (fs.statSync(staged).size) git(checkout, ['apply', '--cached', staged]);
      const root = captured.roots.find(x => inside(wt.source, x.path));
      for (const name of git(checkout, ['ls-files', '-z']).toString().split('\0').filter(Boolean)) {
        const relative = path.join(path.relative(root.path, wt.source), name), item = entries.get(root.id + '/' + relative);
        if (!item) continue; // Preserve working-tree deletions.
        const destination = path.join(checkout, name);
        fs.mkdirSync(path.dirname(destination), { recursive: true });
        if (item.type === 'symlink') fs.symlinkSync(item.target, destination);
        else { fs.copyFileSync(objectPath(item.sha256), destination, fs.constants.COPYFILE_FICLONE); fs.chmodSync(destination, item.mode); }
      }
      for (const [suffix, args] of [['staged', ['diff', '--cached', '--binary', '--full-index']], ['unstaged', ['diff', '--binary', '--full-index']]]) {
        if (!git(checkout, args).equals(fs.readFileSync(path.join(archive, 'git', wt.key + '-' + suffix + '.patch')))) throw new Error('Restored ' + suffix + ' changes differ: ' + wt.source);
      }
      fs.rmSync(checkout, { recursive: true }); restoredWorktrees++;
      progress({ phase: 'restoring-git-worktrees', restoredWorktrees, total: worktrees.length });
    }
    fs.rmSync(restored, { recursive: true });
  }
  fs.rmdirSync(fresh);
  const hashes = {};
  for (const file of fs.readdirSync(path.join(archive, 'git')).filter(x => !x.endsWith('-restored-fsck.txt'))) hashes['git/' + file] = await digest(path.join(archive, 'git', file));
  json(path.join(archive, 'git-hashes.json'), hashes);
  const result = { ...captured, verifiedAt: new Date().toISOString(), verifiedRecords: records, restoredObjects, restoredBytes, restoredBundles: bundles.length, restoredWorktrees, method: 'Every unique payload copied into a fresh directory and read back by SHA-256; all readable file paths checked against their payload inode/size; all bundles freshly cloned, fsck checked and original refs compared. Every tracked worktree reconstructed from its bundle, staged patch and file manifest; staged and unstaged binary diffs compared exactly. Original file modes checked on restored payloads. Original symlink targets retained in manifest; readable links relocated where possible.' };
  json(path.join(archive, 'COMPLETE.json'), result); progress({ phase: 'verified', ...result });
}

if (command === 'capture') await capture();
if (command === 'verify') await verify();
if (command === 'restore-file') {
  const hash = configFile, destination = process.argv[5];
  if (!/^[0-9a-f]{64}$/.test(hash) || !path.isAbsolute(destination ?? '')) throw new Error('Provide a SHA-256 and absolute fresh destination');
  await checkedCopy(objectPath(hash), destination, hash, 0o600);
}
if (command === 'restore-tree') {
  const rootId = configFile, prefix = process.argv[5], destination = process.argv[6];
  if (!path.isAbsolute(destination ?? '') || path.isAbsolute(prefix ?? '') || prefix.split(path.sep).includes('..')) throw new Error('Provide a relative prefix and an absolute fresh destination');
  const captured = JSON.parse(fs.readFileSync(path.join(archive, 'CAPTURED.json'))), root = captured.roots.find(x => x.id === rootId);
  if (!root) throw new Error('Unknown archive root: ' + rootId);
  const source = path.join(root.path, prefix);
  fs.mkdirSync(destination); // Refuse to overlay existing work.
  const directories = []; let restored = 0;
  for await (const line of createInterface({ input: fs.createReadStream(path.join(archive, 'files.jsonl')) })) {
    const item = JSON.parse(line);
    if (item.root !== rootId || (prefix && !inside(item.path, prefix))) continue;
    const target = path.join(destination, path.relative(prefix, item.path));
    if (item.type === 'directory') { fs.mkdirSync(target, { recursive: true }); directories.push([target, item.mode]); }
    if (item.type === 'file') { fs.mkdirSync(path.dirname(target), { recursive: true }); await checkedCopy(objectPath(item.sha256), target, item.sha256, item.mode); }
    if (item.type === 'symlink') {
      const absolute = path.resolve(path.dirname(path.join(root.path, item.path)), item.target);
      const owner = captured.roots.find(x => inside(absolute, x.path));
      let resolved = item.target;
      if (inside(absolute, source)) resolved = path.relative(path.dirname(target), path.join(destination, path.relative(source, absolute)));
      else if (owner) resolved = path.join(archive, 'files', owner.id, path.relative(owner.path, absolute));
      else {
        const inputs = fs.existsSync(path.join(archive, 'external-inputs.json')) ? JSON.parse(fs.readFileSync(path.join(archive, 'external-inputs.json'))).inputs : [];
        const input = inputs.find(x => x.original === absolute);
        if (input) resolved = path.join(archive, input.path);
      }
      fs.mkdirSync(path.dirname(target), { recursive: true }); fs.symlinkSync(resolved, target);
    }
    restored++;
  }
  if (!restored) throw new Error('No preserved entries match this prefix');
  for (const [directory, mode] of directories.reverse()) fs.chmodSync(directory, mode);
  console.log(JSON.stringify({ restored, destination, root: rootId, prefix, note: 'File contents and modes restored. Git metadata is restored separately from its bundle and staged patch. External preserved dependencies link into the read-only archive.' }));
}
