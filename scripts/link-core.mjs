/**
 * Point every workspace package at a local checkout of @stencil/core (for testing core fixes),
 * or back at the published version.
 *
 *   pnpm link:core                       # uses $STENCIL_CORE_PATH or ../../stenciljs/packages/core
 *   pnpm unlink:core
 *
 * Works through a pnpm override in the root package.json, so no package.json under packages/
 * changes. Remember to build core (`pnpm build` in packages/core) after editing it.
 */
import { execSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const pkgPath = path.join(root, 'package.json');
const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'));
const unlink = process.argv.includes('--unlink');

pkg.pnpm ??= {};
pkg.pnpm.overrides ??= {};

if (unlink) {
  delete pkg.pnpm.overrides['@stencil/core'];
  if (!Object.keys(pkg.pnpm.overrides).length) delete pkg.pnpm.overrides;
  if (!Object.keys(pkg.pnpm).length) delete pkg.pnpm;
  console.log('@stencil/core -> published version');
} else {
  const corePath = path.resolve(process.env.STENCIL_CORE_PATH ?? path.join(root, '../../stenciljs/packages/core'));
  if (!existsSync(path.join(corePath, 'package.json'))) {
    console.error(`No @stencil/core at ${corePath} - set STENCIL_CORE_PATH`);
    process.exit(1);
  }
  pkg.pnpm.overrides['@stencil/core'] = `link:${corePath}`;
  console.log(`@stencil/core -> ${corePath}`);
}

writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');
execSync('pnpm install', { cwd: root, stdio: 'inherit' });
