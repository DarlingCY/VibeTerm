const fs = require('fs');
const path = require('path');
const vm = require('vm');

const repoRoot = path.resolve(__dirname, '..');
const uiDir = path.join(repoRoot, 'ui');
const indexPath = path.join(uiDir, 'index.html');
const indexHtml = fs.readFileSync(indexPath, 'utf8');

const assetRefs = [...indexHtml.matchAll(/<(script|link)\b[^>]*(?:src|href)="\.\/([^"]+)"[^>]*>/g)]
  .map(match => match[2]);

let failed = false;

for (const ref of assetRefs) {
  const assetPath = path.join(uiDir, ref);
  if (!fs.existsSync(assetPath)) {
    console.error(`Missing UI asset referenced by index.html: ${ref}`);
    failed = true;
    continue;
  }

  if (ref.endsWith('.js') && !ref.startsWith('vendor/')) {
    try {
      new vm.Script(fs.readFileSync(assetPath, 'utf8'), { filename: ref });
    } catch (error) {
      console.error(`Invalid JavaScript in ${ref}: ${error.message}`);
      failed = true;
    }
  }
}

const appIndex = assetRefs.indexOf('app.js');
const requiredBeforeApp = [
  'startup.js',
  'vendor/xterm/xterm.js',
  'state.js',
  'settings.js',
  'update.js',
  'xterm-support.js',
  'pane-view.js',
  'tabs.js',
];

if (appIndex === -1) {
  console.error('index.html must load app.js');
  failed = true;
}

for (const ref of requiredBeforeApp) {
  const index = assetRefs.indexOf(ref);
  if (index === -1) {
    console.error(`index.html must load ${ref}`);
    failed = true;
  } else if (appIndex !== -1 && index > appIndex) {
    console.error(`${ref} must be loaded before app.js`);
    failed = true;
  }
}

if (failed) {
  process.exit(1);
}

console.log(`UI asset check passed (${assetRefs.length} referenced assets).`);
