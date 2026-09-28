// Builds dist/: one minified CommonJS file for each name Zorvik scripts can
// require(), and THIRD_PARTY_LICENSES.md for everything bundled into them.
// Run with `npm ci && npm run build` (README.md). Zorvik includes the files
// with `include_str!` (crates/script/src/libs.rs), so building Zorvik never
// needs npm.
import * as esbuild from 'esbuild';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const dist = join(root, 'dist');

// Node modules that bundles load at run time through Zorvik's require, so each
// is loaded once, and ones libraries only try (in a try/catch) and do without.
const RUNTIME = ['buffer', 'events', 'stream', 'string_decoder'];

// `name` is what scripts pass to require(); `source` is the entry module.
const LIBRARIES = [
  // Postman's sandbox libraries.
  { name: 'lodash', source: "module.exports = require('lodash');" },
  { name: 'crypto-js', source: "module.exports = require('crypto-js');" },
  { name: 'moment', source: "module.exports = require('moment');" },
  { name: 'ajv', file: 'entries/ajv.js' },
  { name: 'ajv/dist/2019', source: "module.exports = require('ajv').Ajv2019;", external: ['ajv'] },
  { name: 'ajv/dist/2020', source: "module.exports = require('ajv').Ajv2020;", external: ['ajv'] },
  { name: 'ajv-formats', source: "module.exports = require('ajv').addFormats;", external: ['ajv'] },
  { name: 'uuid', file: 'entries/uuid.js' },
  { name: 'tv4', source: "module.exports = require('tv4');" },
  { name: 'chai', source: "module.exports = require('chai');" },
  { name: 'csv-parse/sync', source: "export { parse, CsvError } from 'csv-parse/sync';", buffer: true },
  {
    name: 'csv-parse/lib/sync',
    source: "module.exports = require('csv-parse/sync').parse;",
    external: ['csv-parse/sync'],
  },
  { name: 'xml2js', source: "module.exports = require('xml2js');" },
  { name: 'cheerio', source: "export * from 'cheerio/slim';" },
  // Templates, with the compiler (`Handlebars.compile`); also renders `pm.visualizer`.
  // Without source-map, which it only tries (for precompiling to files).
  { name: 'handlebars', source: "module.exports = require('handlebars');", external: ['source-map'] },
  // Node's own modules, as their browser versions.
  { name: 'buffer', source: "module.exports = require('buffer/');" },
  { name: 'events', source: "module.exports = require('events/');" },
  { name: 'path', source: "module.exports = require('path-browserify');" },
  { name: 'querystring', source: "module.exports = require('querystring-es3');" },
  { name: 'url', source: "module.exports = require('url/');" },
  { name: 'util', source: "module.exports = require('util/');" },
];

// Licenses that may be bundled into Zorvik (MIT OR Apache-2.0).
const ALLOWED = ['MIT', 'ISC', 'BSD-2-Clause', 'BSD-3-Clause', 'Apache-2.0', '0BSD', 'Public Domain'];

/** Package directory, name and version of a bundled file, or null for our own files. */
function packageOf(input) {
  const at = input.lastIndexOf('node_modules/');
  // `(disabled):…` is a module a browser field replaced with nothing.
  if (at < 0 || input.startsWith('(disabled):')) return null;
  const parts = input.slice(at + 'node_modules/'.length).split('/');
  const name = parts[0].startsWith('@') ? parts[0] + '/' + parts[1] : parts[0];
  const dir = join(root, input.slice(0, at), 'node_modules', name);
  const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
  return { dir, name, version: pkg.version, pkg };
}

function licenseOf(pkg) {
  if (typeof pkg.license === 'string') return pkg.license;
  const list = [pkg.license, pkg.licenses].find(Array.isArray);
  if (list) return list.map((l) => l.type || l).join(' OR ');
  if (pkg.license && pkg.license.type) return pkg.license.type;
  return 'UNKNOWN';
}

function allowed(license) {
  return license
    .replace(/[()]/g, '')
    .split(/ OR /)
    .some((l) => ALLOWED.includes(l.trim()));
}

const ISC = `Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.`;

/** The package's license file, else the License section of its README, else the standard ISC text. */
function licenseText(p, license) {
  const files = readdirSync(p.dir);
  const file = files.find((f) => /^(licen[cs]e|copying)([-.][\w.-]*)?$/i.test(f));
  if (file) return readFileSync(join(p.dir, file), 'utf8').trim();
  const readme = files.find((f) => /^readme(\.md)?$/i.test(f));
  const section =
    readme && /^#+ *licen[cs]e\s*\n([\s\S]*?)(?=\n#+ |$(?![\s\S]))/im.exec(readFileSync(join(p.dir, readme), 'utf8'));
  if (section && section[1].trim()) return section[1].trim();
  const author = typeof p.pkg.author === 'string' ? p.pkg.author : p.pkg.author && p.pkg.author.name;
  if (license === 'ISC' && author) return `ISC License\n\nCopyright (c) ${author}\n\n${ISC}`;
  throw new Error(`${p.name}@${p.version} has no license text; add it to build.mjs`);
}

rmSync(dist, { recursive: true, force: true });
const packages = new Map();
const sizes = [];

for (const lib of LIBRARIES) {
  const result = await esbuild.build({
    absWorkingDir: root,
    ...(lib.file
      ? { entryPoints: [lib.file] }
      : { stdin: { contents: lib.source, resolveDir: root, sourcefile: lib.name + '.js' } }),
    bundle: true,
    write: false,
    metafile: true,
    format: 'cjs',
    platform: 'browser',
    target: 'es2022',
    minify: true,
    legalComments: 'none',
    charset: 'utf8',
    define: { global: 'globalThis' },
    external: [...RUNTIME.filter((m) => m !== lib.name), ...(lib.external || [])],
    alias: { timers: './shims/timers.js' },
    inject: ['./shims/process.js', ...(lib.buffer ? ['./shims/buffer.js'] : [])],
    logLevel: 'warning',
  });

  const used = new Map();
  for (const input of Object.keys(result.metafile.inputs)) {
    const p = packageOf(input);
    if (!p || used.has(p.name)) continue;
    const license = licenseOf(p.pkg);
    if (!allowed(license)) throw new Error(`${p.name}@${p.version} has the license "${license}", which Zorvik can't bundle`);
    used.set(p.name, p);
    const known = packages.get(p.name + '@' + p.version);
    if (known) known.usedBy.push(lib.name);
    else packages.set(p.name + '@' + p.version, { ...p, license, text: licenseText(p, license), usedBy: [lib.name] });
  }
  const list = [...used.values()].sort((a, b) => a.name.localeCompare(b.name)).map((p) => `${p.name}@${p.version} (${licenseOf(p.pkg)})`);
  const header =
    `/* Zorvik built-in library "${lib.name}"` +
    (list.length ? `: ${list.join(', ')}` : '') +
    '. Licenses: crates/script/vendor/THIRD_PARTY_LICENSES.md */\n';
  const code = header + result.outputFiles[0].text;
  const out = join(dist, lib.name + '.js');
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, code);
  sizes.push([lib.name, Buffer.byteLength(code)]);
}

// THIRD_PARTY_LICENSES.md
const sorted = [...packages.values()].sort((a, b) => a.name.localeCompare(b.name));
let md = '# Third-party licenses\n\n';
md += 'Zorvik scripts can `require()` the libraries in `dist/`, which are built from these npm packages ';
md += '(generated by `build.mjs`; see README.md).\n\n';
md += '| Package | Version | License | Built into |\n|---|---|---|---|\n';
for (const p of sorted) md += `| ${p.name} | ${p.version} | ${p.license} | ${p.usedBy.join(', ')} |\n`;
for (const p of sorted) {
  md += `\n## ${p.name} ${p.version}\n\n`;
  md += '```text\n' + p.text.replace(/```/g, "'''") + '\n```\n';
}
writeFileSync(join(root, 'THIRD_PARTY_LICENSES.md'), md);

let total = 0;
for (const [name, size] of sizes) {
  total += size;
  console.log(`${name.padEnd(20)} ${(size / 1024).toFixed(1).padStart(7)} KB`);
}
console.log(`${'total'.padEnd(20)} ${(total / 1024).toFixed(1).padStart(7)} KB`);
if (!existsSync(join(dist, 'lodash.js'))) throw new Error('dist/ was not written');
