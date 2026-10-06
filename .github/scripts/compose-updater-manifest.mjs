import fs from 'node:fs';
import path from 'node:path';

const assetsDir = process.env.ASSETS_DIR ?? 'release-assets';
const repo = process.env.GH_REPO ?? '1tuz/Lead-Aggregator';
const tag = process.env.TAG;
const version = process.env.VERSION;
const notes = process.env.NOTES ?? '';
if (!tag || !version) throw new Error('TAG and VERSION are required');

const platforms = {};
// Prefer AppImage for Linux updates: deb packages have no self-update path,
// so when both artifacts exist the AppImage signature must win.
const signatures = fs
  .readdirSync(assetsDir)
  .filter((name) => name.endsWith('.sig'))
  .sort((a, b) => {
    const score = (name) => (/\.appimage/i.test(name) ? 1 : /\.deb$/i.test(name) ? 0 : 0.5);
    return score(b) - score(a);
  });
for (const signatureFile of signatures) {
  const artifact = signatureFile.slice(0, -4);
  const signature = fs.readFileSync(path.join(assetsDir, signatureFile), 'utf8').trim();
  let target = null;
  if (/\.app\.tar\.gz$/i.test(artifact)) {
    // The macos-15 release runner is arm64 and Tauri omits the architecture from this filename.
    target = /x86_64|x64|amd64/i.test(artifact) ? 'darwin-x86_64' : 'darwin-aarch64';
  } else if (/\.appimage(?:\.tar\.gz)?$/i.test(artifact)) {
    target = /aarch64|arm64/i.test(artifact) ? 'linux-aarch64' : 'linux-x86_64';
  } else if (/\.deb$/i.test(artifact)) {
    target = /arm64|aarch64/i.test(artifact) ? 'linux-aarch64' : 'linux-x86_64';
  } else if (/\.msi$/i.test(artifact)) {
    target = /arm64|aarch64/i.test(artifact) ? 'windows-aarch64' : 'windows-x86_64';
  }
  if (!target) continue;
  // Signatures arrive sorted by preference (AppImage before deb), so the
  // first one wins and lower-priority duplicates are skipped silently.
  if (platforms[target]) continue;
  platforms[target] = {
    signature,
    url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(artifact)}`,
  };
}

for (const required of ['darwin-aarch64', 'linux-x86_64', 'windows-x86_64']) {
  if (!platforms[required]) throw new Error(`Missing signed updater asset: ${required}`);
}

fs.writeFileSync(
  path.join(assetsDir, 'latest.json'),
  JSON.stringify({ version, notes, pub_date: new Date().toISOString(), platforms }, null, 2),
);
