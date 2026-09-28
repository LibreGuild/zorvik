use std::time::{Duration, Instant};

use super::LIBRARIES;
use crate::*;

fn pre() -> ScriptInput {
    ScriptInput {
        event: Event::PreRequest,
        info: Info::default(),
        environment_name: None,
        request: ScriptRequest { url: "https://api.test/".into(), method: "GET".into(), ..Default::default() },
        response: None,
        variables: Variables::default(),
        cookies: Vec::new(),
    }
}

/// Console output of a script that must not fail.
fn logs(source: &str) -> Vec<String> {
    let out = run(source, &pre(), &Limits::default());
    assert!(out.error.is_none(), "unexpected error: {:?}\nconsole: {:?}", out.error, out.console);
    out.console.into_iter().map(|c| c.message).collect()
}

/// Error message of a script that must fail.
fn error(source: &str) -> String {
    run(source, &pre(), &Limits::default()).error.unwrap_or_else(|| panic!("`{source}` should fail")).message
}

#[test]
fn every_library_loads() {
    let dist = concat!(env!("CARGO_MANIFEST_DIR"), "/vendor/dist");
    let mut files = Vec::new();
    let mut dirs = vec![std::path::PathBuf::from(dist)];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                let name = path.strip_prefix(dist).unwrap().with_extension("");
                files.push(name.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    files.sort();
    let names: Vec<&str> = LIBRARIES.iter().map(|(name, _)| *name).collect();
    assert_eq!(files, names, "vendor/dist and LIBRARIES list the same libraries, in order");
    for name in names {
        let out = logs(&format!("const m = require('{name}'); console.log(typeof m, m === require('{name}'));"));
        assert!(out == ["function true"] || out == ["object true"], "{name}: {out:?}");
    }
}

#[test]
fn lodash() {
    let out = logs(
        r#"
        const _ = require('lodash');
        console.log(JSON.stringify(_.chunk([1, 2, 3, 4, 5], 2)));
        console.log(_.get({ data: { items: [{ id: 7 }] } }, 'data.items[0].id'), _.get({}, 'a.b', 'none'));
        console.log(JSON.stringify(_.sortBy([{ n: 'b' }, { n: 'a' }], 'n').map((o) => o.n)));
        "#,
    );
    assert_eq!(out, ["[[1,2],[3,4],[5]]", "7 none", r#"["a","b"]"#]);
    // Postman's `_` global, without require.
    assert_eq!(logs("console.log(_.uniq([1, 1, 2]).length, _ === require('lodash'));"), ["2 true"]);
}

#[test]
fn crypto_js() {
    let out = logs(
        r#"
        const CryptoJS = require('crypto-js');
        const mac = CryptoJS.HmacSHA256('message', 'secret');
        console.log(mac.toString(), CryptoJS.enc.Base64.stringify(mac));
        console.log(CryptoJS.SHA256('abc').toString(CryptoJS.enc.Hex).slice(0, 16), CryptoJS.MD5('x').toString());
        console.log(CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('user:pass')));
        // With a passphrase: a random salt, from crypto.getRandomValues.
        const sealed = CryptoJS.AES.encrypt('secret text', 'passphrase').toString();
        console.log(CryptoJS.AES.decrypt(sealed, 'passphrase').toString(CryptoJS.enc.Utf8), sealed !== CryptoJS.AES.encrypt('secret text', 'passphrase').toString());
        // With a key and IV.
        const key = CryptoJS.enc.Hex.parse('000102030405060708090a0b0c0d0e0f');
        const iv = CryptoJS.enc.Hex.parse('0f0e0d0c0b0a09080706050403020100');
        const ct = CryptoJS.AES.encrypt('hello', key, { iv }).ciphertext.toString(CryptoJS.enc.Base64);
        console.log(CryptoJS.AES.decrypt(ct, key, { iv }).toString(CryptoJS.enc.Utf8));
        console.log(CryptoJS === require('crypto-js'));
        "#,
    );
    assert_eq!(
        out,
        [
            "8b5f48702995c1598c573db1e21866a9b825d4a794d169d7060a03605796360b i19IcCmVwVmMVz2x4hhmqbgl1KeU0WnXBgoDYFeWNgs=",
            "ba7816bf8f01cfea 9dd4e461268c8034f5c8564e155c67a6",
            "dXNlcjpwYXNz",
            "secret text true",
            "hello",
            "true"
        ]
    );
}

#[test]
fn moment() {
    let out = logs(
        r#"
        const moment = require('moment');
        const start = moment.utc('2024-01-31T10:00:00Z');
        console.log(start.clone().add(1, 'month').format('YYYY-MM-DD HH:mm'), start.format('ddd, D MMM YYYY'));
        console.log(moment.utc('2024-03-01').diff(moment.utc('2024-02-01'), 'days'), start.toISOString());
        console.log(moment.utc('2024-01-31', 'YYYY-MM-DD').isValid(), moment('not a date', 'YYYY-MM-DD', true).isValid());
        console.log(moment().isValid(), moment.unix(0).utc().format());
        "#,
    );
    assert_eq!(
        out,
        ["2024-02-29 10:00 Wed, 31 Jan 2024", "29 2024-01-31T10:00:00.000Z", "true false", "true 1970-01-01T00:00:00Z"]
    );
}

#[test]
fn ajv() {
    let out = logs(
        r#"
        const Ajv = require('ajv');
        const ajv = new Ajv({ allErrors: true });
        const schema = {
          $schema: 'http://json-schema.org/draft-07/schema#',
          type: 'object',
          required: ['id', 'email'],
          properties: {
            id: { type: 'integer', example: 1 },
            email: { type: 'string', format: 'email' },
            created: { type: 'string', format: 'date-time' },
          },
        };
        console.log(ajv.validate(schema, { id: 1, email: 'a@b.test', created: '2024-01-31T10:00:00Z' }));
        console.log(ajv.validate(schema, { id: 'x', created: 'yesterday' }));
        console.log(ajv.errorsText());
        const validate = ajv.compile({ type: 'array', items: { type: 'number' } });
        console.log(validate([1, 2]), validate([1, '2']), validate.errors[0].instancePath, validate.errors[0].message);
        // The newer drafts.
        const Ajv2020 = require('ajv/dist/2020');
        const tuple = new Ajv2020().compile({ type: 'array', prefixItems: [{ type: 'string' }, { type: 'number' }], items: false });
        console.log(tuple(['a', 1]), tuple(['a', 1, 2]));
        const Ajv2019 = require('ajv/dist/2019');
        console.log(new Ajv2019().validate({ type: 'object', properties: { a: true }, unevaluatedProperties: false }, { a: 1, b: 2 }));
        "#,
    );
    assert_eq!(
        out,
        [
            "true",
            "false",
            "data must have required property 'email', data/id must be integer, data/created must match format \"date-time\"",
            "true false /1 must be number",
            "true false",
            "false"
        ]
    );
    // Strict mode, when asked for, still reports unknown keywords.
    assert!(
        error("new (require('ajv'))({ strict: true }).compile({ type: 'string', example: 'x' });")
            .contains("unknown keyword: \"example\""),
    );
}

#[test]
fn uuid_and_crypto() {
    let out = logs(
        r#"
        const uuid = require('uuid');
        const v4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
        console.log(v4.test(uuid.v4()), v4.test(uuid()), v4.test(crypto.randomUUID()), uuid.v4() !== uuid.v4());
        console.log(uuid.validate(uuid.v7()), uuid.version(uuid.v7()), uuid.v5('zorvik', uuid.v5.URL));
        const bytes = crypto.getRandomValues(new Uint32Array(4));
        console.log(bytes.length, bytes.some((n) => n !== 0));
        "#,
    );
    assert_eq!(out, ["true true true true", "true 7 9d0b6c93-3d3e-597f-a1b6-f2dbbc45cd63", "4 true"]);
    assert_eq!(
        error("crypto.getRandomValues(new Uint8Array(65537))"),
        "RangeError: crypto.getRandomValues: at most 65536 bytes at a time"
    );
    assert_eq!(
        error("crypto.getRandomValues(new Float64Array(2))"),
        "TypeError: crypto.getRandomValues: the argument must be an integer typed array"
    );
}

#[test]
fn tv4() {
    let out = logs(
        r#"
        const schema = { type: 'object', required: ['id'], properties: { id: { type: 'string' } } };
        console.log(tv4.validate({ id: 'a' }, schema), tv4.validate({ id: 1 }, schema), tv4.error.message, tv4.error.dataPath);
        console.log(require('tv4').validateMultiple({}, schema).errors[0].message);
        "#,
    );
    assert_eq!(out, ["true false Invalid type: number (expected string) /id", "Missing required property: id"]);
}

#[test]
fn chai() {
    let out = logs(
        r#"
        const { expect, assert } = require('chai');
        expect({ a: [1, 2] }).to.have.nested.property('a[1]', 2);
        expect([3, 1]).to.have.members([1, 3]).and.lengthOf(2);
        assert.deepEqual({ a: 1 }, { a: 1 });
        try { expect(1).to.equal(2); } catch (e) { console.log(e.name, e.message); }
        pm.test('with chai', () => expect('abc').to.match(/b/));
        "#,
    );
    assert_eq!(out, ["AssertionError expected 1 to equal 2"]);
}

#[test]
fn csv_parse() {
    let out = logs(
        r#"
        const parse = require('csv-parse/lib/sync');
        const csv = 'id,name\n1,"Smith, Ann"\n\n2,Bo\n';
        console.log(JSON.stringify(parse(csv, { columns: true, skip_empty_lines: true })));
        const { parse: parseSync } = require('csv-parse/sync');
        console.log(JSON.stringify(parseSync('a;b\n1;2', { delimiter: ';' })));
        try { parse('a,b\n1', { columns: true }); } catch (e) { console.log(e.code); }
        "#,
    );
    assert_eq!(
        out,
        [
            r#"[{"id":"1","name":"Smith, Ann"},{"id":"2","name":"Bo"}]"#,
            r#"[["a","b"],["1","2"]]"#,
            "CSV_RECORD_INCONSISTENT_COLUMNS"
        ]
    );
}

#[test]
fn xml2js_and_xml2json() {
    let out = logs(
        r#"
        const xml = '<?xml version="1.0"?><user id="7"><name> Ann </name><tag>a</tag><tag>b</tag></user>';
        console.log(JSON.stringify(xml2Json(xml)));
        const xml2js = require('xml2js');
        let parsed;
        xml2js.parseString(xml, (err, result) => { parsed = result; });
        console.log(JSON.stringify(parsed.user.tag), parsed.user.$.id);
        console.log(new xml2js.Builder({ headless: true, renderOpts: { pretty: false } }).buildObject({ a: { b: 1 } }));
        xml2js.parseStringPromise('<a>1</a>').then((r) => console.log('promise', r.a));
        "#,
    );
    assert_eq!(
        out,
        [r#"{"user":{"$":{"id":"7"},"name":"Ann","tag":["a","b"]}}"#, r#"["a","b"] 7"#, "<a><b>1</b></a>", "promise 1"]
    );
    assert!(
        error("xml2Json('<a><b></a>')").starts_with("Error: xml2Json: the text is not valid XML (Unexpected close tag")
    );
}

#[test]
fn cheerio() {
    let out = logs(
        r#"
        const $ = cheerio.load('<html><head><title>Hi</title></head><body><ul><li class="a">x</li><li>y</li></ul></body></html>');
        console.log($('title').text(), $('li.a').text(), $('li').length, $('li').last().text());
        console.log(require('cheerio').load('<p id="p">t</p>')('#p').attr('id'));
        "#,
    );
    assert_eq!(out, ["Hi x 2 y", "p"]);
}

#[test]
fn handlebars() {
    let out = logs(
        r#"
        const Handlebars = require('handlebars');
        const template = Handlebars.compile('<b>{{name}}</b>{{#each items}}<i>{{this}}</i>{{/each}}');
        console.log(template({ name: 'x', items: [1, 2] }));
        // Escaped by default; triple braces don't escape.
        console.log(Handlebars.compile('{{v}}|{{{v}}}')({ v: '<a href="x">&\'</a>' }));
        Handlebars.registerHelper('upper', (s) => String(s).toUpperCase());
        console.log(Handlebars.compile('{{upper name}}{{#if missing}}!{{else}}?{{/if}}')({ name: 'ann' }));
        const table = Handlebars.compile('<table>{{#each rows}}<tr><td>{{@index}}</td><td>{{id}}</td></tr>{{/each}}</table>');
        console.log(table({ rows: [{ id: 7 }, { id: 8 }] }));
        "#,
    );
    assert_eq!(
        out,
        [
            "<b>x</b><i>1</i><i>2</i>",
            "&lt;a href&#x3D;&quot;x&quot;&gt;&amp;&#x27;&lt;/a&gt;|<a href=\"x\">&'</a>",
            "ANN?",
            "<table><tr><td>0</td><td>7</td></tr><tr><td>1</td><td>8</td></tr></table>"
        ]
    );
    assert!(error("require('handlebars').compile('{{#if}}')()").contains("Parse error"));
}

#[test]
fn node_modules() {
    let out = logs(
        r#"
        const url = require('url');
        const u = url.parse('https://api.test:8443/v1/users?page=2&tag=a&tag=b#top', true);
        console.log(u.hostname, u.port, u.pathname, u.query.page, JSON.stringify(u.query.tag), u.hash);
        const qs = require('querystring');
        console.log(qs.stringify({ a: 'x y', b: [1, 2] }), qs.parse('a=1&b=%20').b === ' ');
        const path = require('node:path');
        console.log(path.join('/a/b', '../c', 'd.json'), path.extname('x.tar.gz'), path.basename('/a/b.txt', '.txt'));
        const util = require('util');
        console.log(util.format('%s=%d', 'n', 42), util.inspect({ a: [1] }), typeof util.promisify(() => {}));
        const EventEmitter = require('events');
        const e = new EventEmitter();
        e.once('hi', (n) => console.log('event', n));
        e.emit('hi', 1);
        e.emit('hi', 2);
        const { Buffer } = require('buffer');
        console.log(Buffer.from('user:pass').toString('base64'), Buffer.from('68690a', 'hex').toString().trim());
        // The libraries' `process` and `Buffer` stay theirs.
        console.log(typeof process, typeof globalThis.Buffer, typeof window);
        "#,
    );
    assert_eq!(
        out,
        [
            r#"api.test 8443 /v1/users 2 ["a","b"] #top"#,
            "a=x%20y&b=1&b=2 true",
            "/a/c/d.json .gz b",
            "n=42 { a: [ 1 ] } function",
            "event 1",
            "dXNlcjpwYXNz hi",
            "undefined undefined undefined"
        ]
    );
}

#[test]
fn pm_require_and_unknown_modules() {
    assert_eq!(
        logs(
            "console.log(pm.require('npm:lodash@4.17.21') === require('lodash'), pm.require('npm:moment') === require('moment'), pm.require('csv-parse/sync') === require('csv-parse/sync'));"
        ),
        ["true true true"]
    );
    assert!(
        error("pm.require('npm:@faker-js/faker@8.4.1')").starts_with("Error: Cannot find module '@faker-js/faker'. ")
    );
    let names = LIBRARIES.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", ");
    assert_eq!(
        error("require('left-pad')"),
        format!("Error: Cannot find module 'left-pad'. Scripts can require only these built-in libraries: {names}")
    );
    assert!(error("pm.require('npm:left-pad@1.3.0')").starts_with("Error: Cannot find module 'left-pad'. "));
    assert!(error("require('fs')").starts_with("Error: Cannot find module 'fs'. "));
    assert!(error("require('./helpers.js')").starts_with("Error: Cannot find module './helpers.js'. "));
    assert_eq!(
        error("pm.require('@my-team/helpers')"),
        "Error: pm.require: '@my-team/helpers' is from a Postman package library, which Zorvik can't reach. Copy its code into the script."
    );
    assert_eq!(error("require(1)"), "TypeError: require: the module name must be a string");
    // Errors keep the script's line.
    let out = run("const a = 1;\nrequire('nope');", &pre(), &Limits::default());
    assert_eq!(out.error.unwrap().line, Some(2));
}

#[test]
fn libraries_are_fresh_in_every_run() {
    assert_eq!(logs("require('lodash').marker = 1; console.log(require('lodash').marker);"), ["1"]);
    assert_eq!(logs("console.log(require('lodash').marker, typeof _.chunk);"), ["undefined function"]);
    // Assigning a library global replaces it.
    assert_eq!(logs("_ = 5; CryptoJS = 'x'; console.log(_, CryptoJS);"), ["5 x"]);
}

#[test]
fn library_failures_are_script_errors() {
    // Out of memory while loading: the usual error, not an abort.
    for memory in (512 * 1024..4 * 1024 * 1024).step_by(96 * 1024) {
        for source in ["require('cheerio');", "require('ajv');", "require('lodash'); require('moment');"] {
            if let Some(e) = run(source, &pre(), &Limits { memory, ..Default::default() }).error {
                assert!(e.message.contains("ran out of memory"), "{memory} {source}: {e:?}");
            }
        }
    }
    let out =
        run("require('lodash'); require('moment');", &pre(), &Limits { memory: 1024 * 1024, ..Default::default() });
    assert!(out.error.unwrap().message.contains("ran out of memory (limit 1 MB)"));
    // A broken bundle is reported, not a crash.
    let e = super::compile("broken", "return }").unwrap_err();
    assert!(e.contains("Unexpected token") && e.contains("broken.js:2"), "{e}");
    // The time limit covers loading.
    let limits = Limits { timeout: Duration::from_millis(50), ..Default::default() };
    let out = run("while (true) { require('lodash'); }", &pre(), &limits);
    assert!(out.error.unwrap().message.contains("took longer"));
}

#[test]
fn repeated_requires_are_fast() {
    let source = "require('lodash'); require('moment'); require('crypto-js');";
    run(source, &pre(), &Limits::default()); // compiles the bytecode
    let time = |source: &str| {
        let started = Instant::now();
        for _ in 0..5 {
            assert!(run(source, &pre(), &Limits::default()).error.is_none());
        }
        started.elapsed() / 5
    };
    let bare = time("");
    let with = time(source);
    let added = with.saturating_sub(bare);
    // A few ms in release builds; debug builds run QuickJS unoptimized.
    let limit = if cfg!(debug_assertions) { Duration::from_millis(250) } else { Duration::from_millis(25) };
    assert!(added < limit, "the three libraries added {added:?} per run");
}
