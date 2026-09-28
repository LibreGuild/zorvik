# Built-in script libraries

The JavaScript libraries that Zorvik scripts can `require()`. Each file in `dist/` is one name `require` knows (`dist/csv-parse/lib/sync.js` is `require('csv-parse/lib/sync')`): a minified CommonJS bundle of the npm package and everything it uses. `crates/script/src/libs.rs` builds them into Zorvik with `include_str!`, so building Zorvik never needs npm or the network. The generated files are committed.

| Name | From |
|---|---|
| `lodash`, `crypto-js`, `moment`, `tv4`, `chai`, `xml2js`, `handlebars` | The npm package of the same name (handlebars: its browser build, with the compiler) |
| `ajv` | Ajv 8 with `strict: false` and the standard formats by default, as Postman's Ajv 6 behaves (`entries/ajv.js`); `ajv/dist/2019`, `ajv/dist/2020` and `ajv-formats` are parts of the same bundle |
| `uuid` | uuid, also callable as `uuid()` like uuid 3 (`entries/uuid.js`) |
| `csv-parse/sync`, `csv-parse/lib/sync` | csv-parse's sync API; the second is the function itself, as Postman had it |
| `cheerio` | cheerio's htmlparser2 build (`cheerio/slim`) |
| `buffer`, `events`, `path`, `querystring`, `url`, `util` | Browser versions of Node's modules (`buffer`, `events`, `path-browserify`, `querystring-es3`, `url`, `util`) |

Bundles that use another built-in (`buffer`, `events`) load it through `require` at run time instead of carrying a copy. `process` (`shims/process.js`) is private to each bundle, and Node's `timers` is replaced by `shims/timers.js`.

## Update or add a library

Use Node.js 22 or newer:

```bash
cd crates/script/vendor
npm ci                  # or: npm install <package>@<exact version> --save-exact --save-dev
npm run build           # writes dist/ and THIRD_PARTY_LICENSES.md, and prints the sizes
cargo test -p zorvik-script
```

- Versions are exact in `package.json` and locked in `package-lock.json`, so a build gives the same files.
- To add a name, add it to `LIBRARIES` in `build.mjs` and to `LIBRARIES` in `crates/script/src/libs.rs` (same order as the files; a test checks that they agree), and add a test.
- The build stops if a bundled package's license isn't MIT, ISC, BSD, Apache-2.0 or public domain. `THIRD_PARTY_LICENSES.md` lists every bundled package with its license text; each file in `dist/` names its packages in its first line.
- Libraries must work without Node: no `fs`, `net`, `child_process` or timers. They run in QuickJS inside Zorvik's sandbox, so try what you add with a script that uses it.
- Update the library list in the scripting docs (`docs/pages/src/content/docs/docs/scripting/sandbox.md`).
