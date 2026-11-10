// Tauri runs this before dev/build. Only Windows bundles the CLI sidecar.
import { execFileSync } from 'node:child_process';
import { mkdirSync, copyFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root = fileURLToPath(new URL('../', import.meta.url));
const target = process.env.TAURI_ENV_TARGET_TRIPLE || execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (.+)$/m)[1];
if (target.includes('windows')) {
  const debug = process.argv.includes('--debug');
  execFileSync('cargo', ['build', '--locked', '-p', 'novel-cli', '--target', target, ...(debug ? [] : ['--release'])], { cwd: root, stdio: 'inherit' });
  const destination = path.join(root, 'app/src-tauri/binaries');
  mkdirSync(destination, { recursive: true });
  copyFileSync(path.join(root, `target/${target}/${debug ? 'debug' : 'release'}/novel.exe`), path.join(destination, `novel-${target}.exe`));
}
