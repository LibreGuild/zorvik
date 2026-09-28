use super::*;

fn pre() -> ScriptInput {
    ScriptInput {
        event: Event::PreRequest,
        info: Info {
            request_name: "Get user".into(),
            request_id: "users/get.yaml".into(),
            iteration: 0,
            iteration_count: 1,
        },
        environment_name: Some("Dev".into()),
        request: ScriptRequest {
            url: "https://api.test/users?page=1#top".into(),
            method: "GET".into(),
            headers: vec![ScriptHeader::new("Accept", "application/json")],
            body: String::new(),
        },
        response: None,
        variables: Variables::default(),
        cookies: Vec::new(),
    }
}

fn post(code: u16, status: &str, body: &str) -> ScriptInput {
    ScriptInput {
        event: Event::PostResponse,
        response: Some(ScriptResponse {
            code,
            status: status.into(),
            headers: vec![
                ScriptHeader::new("Content-Type", "application/json; charset=utf-8"),
                ScriptHeader::new("X-Request-Id", "abc"),
            ],
            body: body.into(),
            response_time: 12.5,
            response_size: body.len() as u64,
            events: None,
            cookies: Vec::new(),
        }),
        ..pre()
    }
}

fn run_ok(source: &str, input: &ScriptInput) -> ScriptOutput {
    let out = run(source, input, &Limits::default());
    assert!(out.error.is_none(), "unexpected error: {:?}\nconsole: {:?}", out.error, out.console);
    out
}

fn logs(out: &ScriptOutput) -> Vec<String> {
    out.console.iter().map(|c| c.message.clone()).collect()
}

/// Error message of a single failing `pm.test` around `body`.
fn failure(body: &str, input: &ScriptInput) -> String {
    let out = run_ok(&format!("pm.test('t', () => {{ {body} }});"), input);
    assert_eq!(out.tests.len(), 1, "{:?}", out.tests);
    assert!(!out.tests[0].passed, "`{body}` should fail");
    out.tests[0].error.clone().unwrap_or_default()
}

fn change(scope: Scope, key: &str, value: Option<&str>) -> VariableChange {
    VariableChange { scope, key: key.into(), value: value.map(str::to_string) }
}

#[test]
fn console_and_variables() {
    let mut input = pre();
    input.variables.environment.insert("host".into(), "env.test".into());
    input.variables.collection.insert("host".into(), "ws.test".into());
    input.variables.collection.insert("ver".into(), "v2".into());
    input.variables.globals.insert("g".into(), "global".into());
    let out = run_ok(
        r#"
        console.log('host', pm.variables.get('host'), pm.environment.get('host'), pm.collectionVariables.get('host'));
        console.info({ a: 1, b: [1, 2] });
        console.warn('%s has %d items', 'list', 3);
        console.error(new TypeError('bad'));
        pm.environment.set('token', 'abc');
        pm.environment.set('count', 5);
        pm.environment.set('obj', { x: 1 });
        pm.collectionVariables.set('ver', 'v3');
        pm.globals.unset('g');
        pm.variables.set('local', 'yes');
        console.log(pm.variables.get('local'), pm.environment.get('count') + 1, pm.globals.has('g'), pm.environment.name);
        console.log(pm.variables.replaceIn('https://{{host}}/{{ver}}/{{missing}}'));
        "#,
        &input,
    );
    assert_eq!(
        logs(&out),
        [
            "host env.test env.test ws.test",
            r#"{"a":1,"b":[1,2]}"#,
            "list has 3 items",
            "TypeError: bad",
            "yes 6 false Dev",
            "https://env.test/v3/{{missing}}",
        ]
    );
    let levels: Vec<ConsoleLevel> = out.console.iter().map(|c| c.level).collect();
    assert_eq!(levels[1..4], [ConsoleLevel::Info, ConsoleLevel::Warn, ConsoleLevel::Error]);
    assert_eq!(
        out.variables,
        [
            change(Scope::Environment, "token", Some("abc")),
            change(Scope::Environment, "count", Some("5")),
            change(Scope::Environment, "obj", Some(r#"{"x":1}"#)),
            change(Scope::Collection, "ver", Some("v3")),
            change(Scope::Globals, "g", None),
            change(Scope::Local, "local", Some("yes")),
        ]
    );
    assert!(out.request.is_none(), "the request was not changed");
}

#[test]
fn variable_precedence_and_helpers() {
    let mut input = pre();
    let v = &mut input.variables;
    for (i, map) in
        [&mut v.overrides, &mut v.local, &mut v.environment, &mut v.collection, &mut v.globals].into_iter().enumerate()
    {
        map.insert("a".into(), format!("layer{i}"));
        map.insert(format!("only{i}"), "x".into());
    }
    v.data.insert("a".into(), serde_json::json!("data"));
    v.data.insert("n".into(), serde_json::json!(7));
    let out = run_ok(
        r#"
        console.log(pm.variables.get('a'), pm.iterationData.get('n') + 1, pm.iterationData.has('a'));
        pm.variables.set('a', 'mine');
        console.log(pm.variables.get('a'), Object.keys(pm.variables.toObject()).length);
        console.log(pm.environment.replaceIn('{{a}}-{{only2}}-{{only3}}'), pm.info.requestName, pm.info.eventName);
        console.log(pm.variables.replaceIn('{{$guid}}').length, /^\d+$/.test(pm.variables.replaceIn('{{$timestamp}}')));
        console.log(typeof pm.iterationData.set, pm.info.iteration, pm.info.iterationCount, pm.info.requestId);
        "#,
        &input,
    );
    assert_eq!(
        logs(&out),
        [
            "layer0 8 true",
            "layer0 7",
            "layer2-x-{{only3}} Get user prerequest",
            "36 true",
            "undefined 0 1 users/get.yaml"
        ]
    );

    let mut vars = input.variables.clone();
    assert_eq!(vars.get("n").as_deref(), Some("7"));
    vars.overrides.clear();
    vars.apply(&change(Scope::Local, "a", None));
    assert_eq!(vars.get("a").as_deref(), Some("data"));
    vars.apply(&change(Scope::Globals, "fresh", Some("1")));
    assert_eq!(vars.get("fresh").as_deref(), Some("1"));
}

#[test]
fn pre_request_changes_the_request() {
    let out = run_ok(
        r#"
        pm.request.headers.add({ key: 'X-Trace', value: 'on' });
        pm.request.headers.upsert({ key: 'accept', value: 'text/plain' });
        pm.request.headers.add('X-Two: 2');
        pm.request.headers.remove('x-two');
        pm.request.addHeader({ key: 'X-Sig', value: 42 });
        console.log(pm.request.headers.get('ACCEPT'), pm.request.headers.has('X-Trace'), pm.request.headers.count());
        console.log(pm.request.url.toString(), pm.request.url.getHost(), pm.request.url.getPath());
        console.log(pm.request.url.query.get('page'), pm.request.url.protocol, pm.request.url.path.join('|'));
        pm.request.url.query.add({ key: 'q', value: 'x y' });
        pm.request.url.query.upsert({ key: 'page', value: '2' });
        console.log(`${pm.request.url}`, pm.request.url.getQueryString());
        pm.request.method = 'post';
        pm.request.body.raw = JSON.stringify({ id: 1 });
        console.log(pm.request.body.mode, pm.request.body.raw, pm.request.method);
        "#,
        &pre(),
    );
    assert_eq!(
        logs(&out),
        [
            "text/plain true 3",
            "https://api.test/users?page=1#top api.test /users",
            "1 https users",
            "https://api.test/users?page=2&q=x y#top page=2&q=x y",
            r#"raw {"id":1} POST"#,
        ]
    );
    let request = out.request.expect("changed");
    assert_eq!(request.url, "https://api.test/users?page=2&q=x y#top");
    assert_eq!(request.method, "POST");
    assert_eq!(request.body, r#"{"id":1}"#);
    assert_eq!(
        request.headers,
        [
            ScriptHeader::new("Accept", "text/plain"),
            ScriptHeader::new("X-Trace", "on"),
            ScriptHeader::new("X-Sig", "42")
        ]
    );

    let out = run_ok("pm.request.url = pm.request.url.toString().replace('users', 'people');", &pre());
    assert_eq!(out.request.unwrap().url, "https://api.test/people?page=1#top");
    let out = run_ok("pm.request.url.update('{{base}}/x'); console.log(pm.request.url.getHost());", &pre());
    assert_eq!(logs(&out), ["{{base}}"]);
    assert_eq!(out.request.unwrap().url, "{{base}}/x");
}

#[test]
fn response_and_tests() {
    let body = r#"{"id": 7, "name": "Ada", "tags": ["a", "b"], "nested": {"deep": [1, {"x": true}]}}"#;
    let out = run_ok(
        r#"
        pm.test('status', () => pm.response.to.have.status(200));
        pm.test('reason', () => pm.response.to.have.status('OK'));
        pm.test('ok', () => { pm.response.to.be.ok; pm.response.to.be.success; pm.response.to.not.be.error; });
        pm.test('json', () => { pm.response.to.be.json; pm.response.to.have.jsonBody('nested.deep[1].x', true); });
        pm.test('header', () => { pm.response.to.have.header('content-type'); pm.response.to.have.header('X-Request-Id', 'abc'); });
        pm.test('body', () => {
            const json = pm.response.json();
            pm.expect(json.name).to.eql('Ada');
            pm.expect(pm.response.text()).to.include('Ada');
            pm.expect(pm.response.code).to.equal(200);
            pm.expect(pm.response.status).to.equal('OK');
            pm.expect(pm.response.responseTime).to.be.below(100);
            pm.expect(pm.response.responseSize).to.be.above(10);
            pm.expect(pm.response.headers.get('X-REQUEST-ID')).to.equal('abc');
            pm.expect(pm.response).to.have.status(200);
        });
        pm.test('fails', () => pm.expect(pm.response.code).to.equal(201));
        pm.test('throws', () => { null.x; });
        pm.test.skip('later', () => { throw new Error('not run'); });
        pm.test('no function');
        tests['legacy'] = responseCode.code === 200;
        tests['legacy fail'] = false;
        "#,
        &post(200, "OK", body),
    );
    let summary: Vec<(&str, bool, bool)> = out.tests.iter().map(|t| (t.name.as_str(), t.passed, t.skipped)).collect();
    assert_eq!(
        summary,
        [
            ("status", true, false),
            ("reason", true, false),
            ("ok", true, false),
            ("json", true, false),
            ("header", true, false),
            ("body", true, false),
            ("fails", false, false),
            ("throws", false, false),
            ("later", false, true),
            ("no function", false, true),
            ("legacy", true, false),
            ("legacy fail", false, false),
        ]
    );
    assert_eq!(out.tests[6].error.as_deref(), Some("expected 200 to equal 201"));
    assert!(out.tests[7].error.as_deref().unwrap().starts_with("TypeError: "), "{:?}", out.tests[7]);
}

#[test]
fn response_assertion_messages() {
    let input = post(404, "Not Found", "not json");
    for (body, message) in [
        ("pm.response.to.have.status(200)", "expected response to have status code 200 but got 404"),
        ("pm.response.to.not.have.status(404)", "expected response to not have status code 404"),
        ("pm.response.to.have.status('OK')", "expected response to have status reason 'OK' but got 'Not Found'"),
        ("pm.response.to.be.ok", "expected response code to be 200 but found 404"),
        ("pm.response.to.be.success", "expected response code to be 2XX but found 404"),
        ("pm.response.to.not.be.clientError", "expected response code to not be 4XX"),
        ("pm.response.to.have.header('X-Nope')", "expected response to have header with key 'X-Nope'"),
        (
            "pm.response.to.have.header('X-Request-Id', 'x')",
            "expected 'X-Request-Id' response header to be 'x' but got 'abc'",
        ),
        ("pm.expect(pm.response).to.have.status(201)", "expected response to have status code 201 but got 404"),
    ] {
        assert_eq!(failure(body, &input), message, "{body}");
    }
    assert!(failure("pm.response.to.be.json", &input).starts_with("expected response body to be a valid json"));
    assert!(failure("pm.response.json()", &input).contains("not valid JSON"));
}

#[test]
fn expect_passes() {
    let checks = [
        "pm.expect(1).to.equal(1)",
        "pm.expect(1).to.not.equal(2)",
        "pm.expect({ a: [1, { b: 2 }] }).to.eql({ a: [1, { b: 2 }] })",
        "pm.expect({ a: 1 }).to.deep.equal({ a: 1 })",
        "pm.expect('x').to.be.a('string')",
        "pm.expect([]).to.be.an('array').that.is.empty",
        "pm.expect({}).to.be.an('object')",
        "pm.expect(null).to.be.null",
        "pm.expect(undefined).to.be.undefined",
        "pm.expect(NaN).to.be.NaN",
        "pm.expect(true).to.be.true",
        "pm.expect(false).to.be.false",
        "pm.expect(1).to.be.ok",
        "pm.expect(0).to.not.be.ok",
        "pm.expect('').to.exist",
        "pm.expect(null).to.not.exist",
        "pm.expect('abc').to.include('b')",
        "pm.expect([1, 2, 3]).to.contain(2)",
        "pm.expect([{ a: 1 }]).to.deep.include({ a: 1 })",
        "pm.expect({ a: 1, b: 2 }).to.include({ a: 1 })",
        "pm.expect({ a: { x: 1 } }).to.deep.include({ a: { x: 1 } })",
        "pm.expect(5).to.be.above(3).and.below(10)",
        "pm.expect(5).to.be.at.least(5)",
        "pm.expect(5).to.be.at.most(5)",
        "pm.expect(5).to.be.gt(4).and.gte(5).and.lt(6).and.lte(5)",
        "pm.expect(5).to.be.within(1, 10)",
        "pm.expect({ a: { b: [1, 2] } }).to.have.property('a').that.has.property('b')",
        "pm.expect({ a: 1 }).to.have.property('a', 1)",
        "pm.expect({ a: { b: [1, { c: 3 }] } }).to.have.nested.property('a.b[1].c', 3)",
        "pm.expect({ a: 1 }).to.have.own.property('a')",
        "pm.expect({ a: [1] }).to.have.deep.property('a', [1])",
        "pm.expect([1, 2]).to.have.lengthOf(2)",
        "pm.expect('abc').to.have.length(3)",
        "pm.expect([1, 2, 3]).to.have.length.above(2)",
        "pm.expect('hello world').to.match(/world$/)",
        "pm.expect('b').to.be.oneOf(['a', 'b'])",
        "pm.expect({ a: 1, b: 2 }).to.have.all.keys('a', 'b')",
        "pm.expect({ a: 1, b: 2 }).to.have.keys(['a', 'b'])",
        "pm.expect({ a: 1, b: 2 }).to.have.any.keys('a', 'z')",
        "pm.expect({ a: 1, b: 2 }).to.include.keys('a')",
        "pm.expect({ a: 1, b: 2 }).to.contain.all.keys('a')",
        "pm.expect([1, 2, 3]).to.have.members([3, 2, 1])",
        "pm.expect([1, 2, 3]).to.include.members([2])",
        "pm.expect([{ a: 1 }]).to.have.deep.members([{ a: 1 }])",
        "pm.expect([1, 2]).to.have.ordered.members([1, 2])",
        "pm.expect(new Date()).to.be.instanceOf(Date)",
        "pm.expect(1.05).to.be.closeTo(1, 0.1)",
        "pm.expect('foobar').to.have.string('bar')",
        "pm.expect(() => { throw new TypeError('boom') }).to.throw(TypeError, 'boom')",
        "pm.expect(() => {}).to.not.throw()",
        "pm.expect(4).to.satisfy(n => n % 2 === 0)",
        "pm.expect('x', 'custom message').to.equal('x')",
        "pm.expect([1, 2]).to.be.an('array').with.lengthOf(2)",
        "pm.expect(new Map([['a', 1]])).to.have.keys('a')",
        "pm.expect(new Set([1])).to.include(1)",
        "pm.expect(3).to.be.finite",
    ];
    let source: String =
        checks.iter().enumerate().map(|(i, c)| format!("pm.test('{i}', () => {{ {c}; }});\n")).collect();
    let out = run_ok(&source, &pre());
    assert_eq!(out.tests.len(), checks.len());
    for (t, check) in out.tests.iter().zip(checks) {
        assert!(t.passed, "{check}: {:?}", t.error);
    }
}

#[test]
fn expect_failure_messages() {
    let input = pre();
    for (body, message) in [
        ("pm.expect(404).to.equal(200)", "expected 404 to equal 200"),
        ("pm.expect(1).to.not.equal(1)", "expected 1 to not equal 1"),
        ("pm.expect('a').to.be.a('number')", "expected 'a' to be a number"),
        ("pm.expect([1]).to.be.an('object')", "expected [ 1 ] to be an object"),
        ("pm.expect({ a: 1 }).to.eql({ a: 2 })", "expected { a: 1 } to deeply equal { a: 2 }"),
        ("pm.expect(5).to.be.above(10)", "expected 5 to be above 10"),
        ("pm.expect(5).to.not.be.below(10)", "expected 5 to be at least 10"),
        ("pm.expect(undefined).to.be.above(1)", "expected undefined to be a number or a date"),
        ("pm.expect([1, 2]).to.have.lengthOf(3)", "expected [ 1, 2 ] to have a length of 3 but got 2"),
        ("pm.expect([1, 2]).to.have.length.above(5)", "expected [ 1, 2 ] to have a length above 5 but got 2"),
        ("pm.expect({ a: 1 }).to.have.property('b')", "expected { a: 1 } to have property 'b'"),
        ("pm.expect({ a: 1 }).to.have.property('a', 2)", "expected { a: 1 } to have property 'a' of 2, but got 1"),
        ("pm.expect('abc').to.include('z')", "expected 'abc' to include 'z'"),
        ("pm.expect([1, 2]).to.not.include(2)", "expected [ 1, 2 ] to not include 2"),
        ("pm.expect(null).to.exist", "expected null to exist"),
        ("pm.expect('').to.not.be.empty", "expected '' not to be empty"),
        ("pm.expect(0).to.be.ok", "expected 0 to be truthy"),
        ("pm.expect(1).to.be.true", "expected 1 to be true"),
        ("pm.expect('x').to.be.oneOf(['a', 'b'])", "expected 'x' to be one of [ 'a', 'b' ]"),
        ("pm.expect({ a: 1, b: 2 }).to.have.all.keys('a')", "expected { a: 1, b: 2 } to have key 'a'"),
        ("pm.expect({ a: 1 }).to.have.keys('a', 'b')", "expected { a: 1 } to have keys 'a', and 'b'"),
        ("pm.expect([1, 2]).to.have.members([1, 3])", "expected [ 1, 2 ] to have the same members as [ 1, 3 ]"),
        ("pm.expect('abc').to.match(/z/)", "expected 'abc' to match /z/"),
        ("pm.expect(1.5).to.be.closeTo(1, 0.1)", "expected 1.5 to be close to 1 +/- 0.1"),
        ("pm.expect(1, 'status check').to.equal(2)", "status check: expected 1 to equal 2"),
        ("pm.expect(() => {}).to.throw()", "expected [Function] to throw an error"),
        ("pm.expect(5).to.be.within(1, 3)", "expected 5 to be within 1..3"),
        (
            "pm.expect({ id: 1, name: 'a long enough name', email: 'x@example.com' }).to.eql({})",
            "expected { Object (id, name, ...) } to deeply equal {}",
        ),
        ("throw 'plain'", "plain"),
    ] {
        assert_eq!(failure(body, &input), message, "{body}");
    }
}

#[test]
fn async_tests() {
    let out = run_ok(
        r#"
        pm.test('promise ok', async () => { await Promise.resolve(1); pm.expect(1).to.equal(1); });
        pm.test('promise fails', async () => { await null; pm.expect(1).to.equal(2); });
        pm.test('done ok', (done) => { Promise.resolve().then(() => done()); });
        pm.test('done error', (done) => { done(new Error('nope')); });
        pm.test('done never', (done) => {});
        "#,
        &pre(),
    );
    let summary: Vec<(bool, Option<&str>)> = out.tests.iter().map(|t| (t.passed, t.error.as_deref())).collect();
    assert_eq!(
        summary,
        [
            (true, None),
            (false, Some("expected 1 to equal 2")),
            (true, None),
            (false, Some("Error: nope")),
            (false, Some("The test did not finish: done() was never called")),
        ]
    );
}

#[test]
fn errors_have_lines() {
    let out = run("const a = 1;\n\nfoo.bar();\n", &pre(), &Limits::default());
    assert_eq!(out.error, Some(ScriptError { message: "ReferenceError: foo is not defined".into(), line: Some(3) }));

    let out = run("let x = ;", &pre(), &Limits::default());
    let error = out.error.unwrap();
    assert!(error.message.starts_with("SyntaxError"), "{error:?}");
    assert_eq!(error.line, Some(1));

    let out = run(
        "console.log('before');\npm.environment.set('a', '1');\npm.expect(1).to.equal(2);\nconsole.log('after');",
        &pre(),
        &Limits::default(),
    );
    assert_eq!(out.error, Some(ScriptError { message: "AssertionError: expected 1 to equal 2".into(), line: Some(3) }));
    assert_eq!(logs(&out), ["before"]);
    assert_eq!(out.variables, [change(Scope::Environment, "a", Some("1"))]);

    let out = run("function f() {\n  throw new Error('inner');\n}\nf();", &pre(), &Limits::default());
    assert_eq!(out.error, Some(ScriptError { message: "Error: inner".into(), line: Some(2) }));

    let out = run("throw { code: 1 };", &pre(), &Limits::default());
    assert_eq!(out.error.unwrap().message, r#"Uncaught {"code":1}"#);

    let out = run("pm.response.code", &pre(), &Limits::default());
    assert!(out.error.unwrap().message.starts_with("TypeError"));
}

#[test]
fn unsupported_apis_throw_clearly() {
    let out = run("pm.vault.get('a')", &pre(), &Limits::default());
    assert_eq!(
        out.error.map(|e| (e.message, e.line)),
        Some(("Error: pm.vault is not supported in Zorvik".to_string(), Some(1)))
    );
}

/// The app's side for tests: answers sends with an echo of the request, keeps cookies in memory.
#[derive(Default)]
struct FakeHost {
    sent: std::sync::Mutex<Vec<HostRequest>>,
    jar: std::sync::Mutex<Vec<ScriptCookie>>,
}

impl Host for FakeHost {
    fn send(&self, request: HostRequest, timeout: Duration) -> Result<HostResponse, String> {
        assert!(timeout > Duration::ZERO);
        if request.url.contains("down") {
            return Err("Could not connect to down.test".into());
        }
        self.sent.lock().unwrap().push(request.clone());
        let body = serde_json::json!({ "echo": request.method, "url": request.url, "body": request.body }).to_string();
        Ok(HostResponse {
            code: 201,
            status: "Created".into(),
            headers: vec![ScriptHeader::new("Content-Type", "application/json")],
            response_size: body.len() as u64,
            body,
            response_time: 3.0,
            cookies: vec![ScriptCookie { name: "sid".into(), value: "s1".into(), ..Default::default() }],
        })
    }
    fn cookies(&self, _url: &str) -> Result<Vec<ScriptCookie>, String> {
        Ok(self.jar.lock().unwrap().clone())
    }
    fn set_cookie(&self, url: &str, name: &str, value: &str) -> Result<(), String> {
        if url.contains("other.test") {
            return Err("Scripts can only change cookies of the request's own site".into());
        }
        self.jar.lock().unwrap().push(ScriptCookie { name: name.into(), value: value.into(), ..Default::default() });
        Ok(())
    }
    fn remove_cookies(&self, _url: &str, name: Option<&str>) -> Result<(), String> {
        self.jar.lock().unwrap().retain(|c| name.is_some_and(|n| n != c.name));
        Ok(())
    }
    fn dynamic(&self, expression: &str) -> Option<String> {
        (expression == "$randomInt(5, 5)").then(|| "5".into())
    }
}

fn run_host(source: &str, input: &ScriptInput, host: &Arc<FakeHost>) -> ScriptOutput {
    let out = run_with(source, input, &Limits::default(), Some(host.clone() as Arc<dyn Host>));
    assert!(out.error.is_none(), "unexpected error: {:?}\nconsole: {:?}", out.error, out.console);
    out
}

fn env(out: &ScriptOutput, key: &str) -> Option<String> {
    out.variables.iter().find(|c| c.key == key).and_then(|c| c.value.clone())
}

#[test]
fn send_request_with_a_callback_or_await() {
    let host = Arc::new(FakeHost::default());
    let out = run_host(
        r#"pm.sendRequest({
            url: 'https://api.test/login', method: 'post', header: { 'X-Trace': '1' },
            body: { mode: 'raw', raw: JSON.stringify({ user: 'ada' }), options: { raw: { language: 'json' } } },
        }, (err, res) => {
            pm.environment.set('code', String(res.code));
            pm.environment.set('cookie', res.cookies.get('sid'));
            pm.test('echoed', () => pm.expect(res.json().echo).to.eql('POST'));
        });"#,
        &pre(),
        &host,
    );
    assert_eq!(env(&out, "code").as_deref(), Some("201"));
    assert_eq!(env(&out, "cookie").as_deref(), Some("s1"));
    assert!(out.tests[0].passed, "{:?}", out.tests);
    let sent = host.sent.lock().unwrap().clone();
    assert_eq!((sent[0].method.as_str(), sent[0].body.as_str()), ("POST", r#"{"user":"ada"}"#));
    assert!(sent[0].headers.iter().any(|h| h.key == "Content-Type" && h.value == "application/json"));
    assert!(out.console.iter().any(|c| c.message == "pm.sendRequest POST https://api.test/login → 201 Created"));

    let out = run_host(
        "const res = await pm.sendRequest('https://api.test/a');\npm.environment.set('url', res.json().url);",
        &pre(),
        &host,
    );
    assert_eq!(env(&out, "url").as_deref(), Some("https://api.test/a"));

    let out = run_host(
        "pm.sendRequest({ url: 'https://api.test/f', method: 'POST', body: { mode: 'urlencoded', urlencoded: [{ key: 'a b', value: 'c&d' }, { key: 'x', value: '1', disabled: true }] } }, () => {});",
        &pre(),
        &host,
    );
    assert!(out.error.is_none());
    assert_eq!(host.sent.lock().unwrap().last().unwrap().body, "a+b=c%26d");
}

#[test]
fn send_request_failures_reach_the_script() {
    let host = Arc::new(FakeHost::default());
    let out = run_host(
        "pm.sendRequest('https://down.test', (err, res) => { pm.environment.set('err', err.message); pm.environment.set('res', String(res)); });",
        &pre(),
        &host,
    );
    assert_eq!(env(&out, "err").as_deref(), Some("Could not connect to down.test"));
    assert_eq!(env(&out, "res").as_deref(), Some("undefined"));
    let out = run_with("await pm.sendRequest('https://down.test');", &pre(), &Limits::default(), Some(host.clone()));
    assert_eq!(out.error.unwrap().message, "Error: Could not connect to down.test");
    let out = run(
        "pm.sendRequest('https://x.test', (err) => pm.environment.set('e', err.message));",
        &pre(),
        &Limits::default(),
    );
    assert_eq!(env(&out, "e").as_deref(), Some("pm.sendRequest isn't available here"));
}

#[test]
fn timers_run_after_the_script() {
    let started = Instant::now();
    let out = run_ok(
        "let n = 0;\nsetTimeout(() => pm.environment.set('late', 'yes'), 40);\nconst id = setInterval(() => { n++; if (n === 3) { clearInterval(id); pm.environment.set('ticks', String(n)); } }, 5);\nconst never = setTimeout(() => pm.environment.set('never', '1'), 10);\nclearTimeout(never);",
        &pre(),
    );
    assert!(started.elapsed() >= Duration::from_millis(40));
    assert_eq!(env(&out, "late").as_deref(), Some("yes"));
    assert_eq!(env(&out, "ticks").as_deref(), Some("3"));
    assert_eq!(env(&out, "never"), None);
    let out = run("setTimeout(() => { throw new Error('in a timer'); }, 1);", &pre(), &Limits::default());
    assert_eq!(out.error.unwrap().message, "Error: in a timer");
    let limits = Limits { timeout: Duration::from_millis(200), ..Default::default() };
    let out = run("setInterval(() => {}, 10);", &pre(), &limits);
    assert!(out.error.unwrap().message.contains("took longer"));
}

#[test]
fn errors_in_promise_callbacks_are_reported() {
    let out = run("Promise.resolve().then(() => { throw new Error('late'); });", &pre(), &Limits::default());
    assert_eq!(out.error.map(|e| e.message), Some("Error: late".into()));
}

#[test]
fn cookies_and_the_jar() {
    let host = Arc::new(FakeHost::default());
    let mut input = pre();
    input.cookies =
        vec![ScriptCookie { name: "sid".into(), value: "abc".into(), domain: "api.test".into(), ..Default::default() }];
    let out = run_host(
        r#"pm.environment.set('sid', pm.cookies.get('sid'));
        pm.environment.set('has', String(pm.cookies.has('sid', 'abc')));
        const jar = pm.cookies.jar();
        jar.set('https://api.test', 'theme', 'dark', (err) => {
            jar.get('https://api.test', 'theme', (e, value) => pm.environment.set('theme', value));
        });
        jar.set('https://other.test', 'x', '1', (err) => pm.environment.set('refused', err.message));"#,
        &input,
        &host,
    );
    assert_eq!(env(&out, "sid").as_deref(), Some("abc"));
    assert_eq!(env(&out, "has").as_deref(), Some("true"));
    assert_eq!(env(&out, "theme").as_deref(), Some("dark"));
    assert!(env(&out, "refused").unwrap().contains("own site"));
}

#[test]
fn visualizer_renders_handlebars() {
    let out = run_ok(
        "pm.visualizer.set('<b>{{name}}</b><ul>{{#each items}}<li>{{this}}</li>{{/each}}</ul>', { name: '<x>', items: [1, 2] });",
        &post(200, "OK", "{}"),
    );
    assert_eq!(out.visualization.as_deref(), Some("<b>&lt;x&gt;</b><ul><li>1</li><li>2</li></ul>"));
    let out = run_ok("pm.visualizer.set('<i></i>'); pm.visualizer.clear();", &post(200, "OK", "{}"));
    assert_eq!(out.visualization, None);
}

#[test]
fn skip_request_only_before_sending() {
    assert!(run_ok("pm.execution.skipRequest();", &pre()).skip_request);
    assert!(!run_ok("pm.execution.skipRequest();", &post(200, "OK", "")).skip_request);
}

#[test]
fn dynamic_variables_come_from_the_app() {
    let host = Arc::new(FakeHost::default());
    let out =
        run_host("pm.environment.set('n', pm.variables.replaceIn('{{$randomInt(5, 5)}}-{{$guid}}'));", &pre(), &host);
    let n = env(&out, "n").unwrap();
    assert!(n.starts_with("5-") && n.len() == 38, "{n}");
}

#[test]
fn set_next_request_is_reported() {
    assert_eq!(run_ok("pm.test('x', () => {});", &post(200, "OK", "")).next_request, None);
    let out = run_ok("pm.execution.setNextRequest('Login');\npm.execution.setNextRequest(42);", &post(200, "OK", ""));
    assert_eq!(out.next_request, Some(NextRequest { name: Some("42".into()) }), "the last call wins");
    let out = run_ok("postman.setNextRequest(null);", &pre());
    assert_eq!(out.next_request, Some(NextRequest { name: None }));
    // Kept when the script fails afterwards.
    let out = run("pm.execution.setNextRequest('Next'); nope();", &pre(), &Limits::default());
    assert!(out.error.is_some());
    assert_eq!(out.next_request, Some(NextRequest { name: Some("Next".into()) }));
}

#[test]
fn sandbox_has_no_host_access() {
    let out = run_ok(
        r#"
        console.log(typeof std, typeof os, typeof process, typeof fetch, typeof XMLHttpRequest, typeof print);
        let imported = 'no';
        import('os').then(() => { imported = 'yes'; }, () => { imported = 'failed'; });
        Promise.resolve().then(() => Promise.resolve()).then(() => console.log('import', imported));
        console.log(btoa('user:pass'), atob('dXNlcjpwYXNz'), typeof JSON.parse, Math.max(1, 2), typeof Date.now());
        "#,
        &pre(),
    );
    assert_eq!(
        logs(&out),
        [
            "undefined undefined undefined undefined undefined undefined",
            "dXNlcjpwYXNz user:pass function 2 number",
            "import failed"
        ]
    );
}

#[test]
fn legacy_postman_api() {
    let out = run_ok(
        r#"
        postman.setEnvironmentVariable('t', 'v');
        postman.setGlobalVariable('g', 1);
        console.log(postman.getEnvironmentVariable('t'), environment.x, responseBody.length, postman.getResponseHeader('x-request-id'));
        "#,
        &{
            let mut i = post(200, "OK", "{}");
            i.variables.environment.insert("x".into(), "1".into());
            i
        },
    );
    assert_eq!(logs(&out), ["v 1 2 abc"]);
    assert_eq!(out.variables, [change(Scope::Environment, "t", Some("v")), change(Scope::Globals, "g", Some("1"))]);
}

#[test]
fn top_level_return_and_strictness() {
    let out = run_ok("undeclared = 1; if (undeclared) return; console.log('not reached');", &pre());
    assert!(out.console.is_empty());
}

#[test]
fn time_limit_stops_endless_loops() {
    let limits = Limits { timeout: Duration::from_millis(200), ..Default::default() };
    let started = Instant::now();
    let out = run("pm.test('ran', () => {}); console.log('start'); while (true) {}", &pre(), &limits);
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    let error = out.error.clone().unwrap();
    assert_eq!(error.message, "The script took longer than 0.2 s and was stopped");
    assert_eq!(error.line, Some(1));
    // What happened before the stop is kept.
    assert_eq!(logs(&out), ["start"]);
    assert_eq!(out.tests.len(), 1);

    // An endless chain of promise callbacks is stopped too. (Not one that returns
    // each promise: that one grows until it runs out of memory, maybe before the time limit.)
    let out = run("function spin() { Promise.resolve().then(spin); } spin();", &pre(), &limits);
    assert!(out.error.unwrap().message.contains("took longer"));
}

#[test]
fn memory_limit_stops_runaway_allocation() {
    let limits = Limits { memory: 16 * 1024 * 1024, ..Default::default() };
    let out = run("const a = []; while (true) a.push('x'.repeat(1024) + a.length);", &pre(), &limits);
    assert!(out.error.unwrap().message.contains("ran out of memory (limit 16 MB)"));
    let out = run("const o = {}; let i = 0; while (true) o['k' + i] = { i: i++ };", &pre(), &limits);
    let error = out.error.unwrap();
    assert!(error.message.contains("ran out of memory (limit 16 MB)"), "{error:?}");
    let out = run("function f() { return f() + 1; } f();", &pre(), &Limits::default());
    assert!(out.error.unwrap().message.starts_with("RangeError"), "stack overflow is a normal error");
}

#[test]
fn console_is_bounded() {
    let out = run_ok("for (let i = 0; i < 1500; i++) console.log(i); console.log('x'.repeat(20000));", &pre());
    assert_eq!(out.console.len(), 1001);
    assert_eq!(out.console.last().unwrap().message, "501 more console messages were not kept (limit 1000).");
    let out = run_ok("console.log('x'.repeat(20000)); console.log({ self: null }, [1, 'a']);", &pre());
    assert!(out.console[0].message.len() < 10 * 1024 + 50);
    assert!(out.console[0].message.ends_with("… (truncated)"));
    assert_eq!(out.console[1].message, r#"{"self":null} [1,"a"]"#);
    let out = run_ok("const o = { a: 1 }; o.self = o; console.log(o);", &pre());
    assert_eq!(logs(&out), [r#"{"a":1,"self":"[Circular]"}"#]);
}

#[test]
fn big_response_bodies_are_cut() {
    let limits = Limits { memory: 8 * 1024 * 1024, ..Default::default() };
    let body = "y".repeat(3 * 1024 * 1024);
    let out = run("console.log(pm.response.text().length);", &post(200, "OK", &body), &limits);
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(logs(&out)[0], (2 * 1024 * 1024).to_string());
    assert!(out.console[1].message.contains("larger than 2 MB"));
}

#[test]
fn script_line_parsing() {
    assert_eq!(script_line("    at f (prelude.js:10:5)\n    at <anonymous> (script:4:12)\n"), Some(4));
    assert_eq!(script_line("    at script:7:1\n"), Some(7));
    assert_eq!(script_line("    at f (prelude.js:1:1)\n"), None);
    assert_eq!(cut("héllo", 2), "h");
    assert_eq!(seconds(Duration::from_secs(5)), "5 s");
}

#[test]
fn collecting_results_has_a_time_limit() {
    let limits = Limits { timeout: Duration::from_millis(200), ..Default::default() };
    for source in [
        "tests = new Proxy({}, { ownKeys() { while (true) {} } });",
        "tests = { get slow() { while (true) {} } };",
        "Array.prototype.push = function () { while (true) {} };",
        "pm.environment.set('a', '1'); Map.prototype.forEach = function () { while (true) {} };",
    ] {
        let started = Instant::now();
        let out = run(source, &pre(), &limits);
        assert!(started.elapsed() < Duration::from_secs(5), "{source}: {:?}", started.elapsed());
        let error = out.error.unwrap_or_else(|| panic!("{source}: no error"));
        assert!(error.message.starts_with("Collecting the script's results took too long"), "{source}: {error:?}");
    }
}

#[test]
fn out_of_memory_in_built_ins_does_not_abort() {
    // Running out of memory inside JSON.stringify leaks an object in QuickJS; with
    // assertions on, freeing the runtime then aborted the process (hit near 700 KB).
    let script = "const big = []; for (let i = 0; i < 2000; i++) big.push({ i, s: 'v' + i }); JSON.stringify(big);";
    for memory in (640 * 1024..800 * 1024).step_by(997) {
        run(script, &pre(), &Limits { memory, ..Default::default() });
    }
    // A response too big for the memory limit: an out-of-memory error, not a confusing TypeError.
    let body = format!("[{}]", vec![r#"{"id":1,"name":"item","tags":["a","b"]}"#; 40_000].join(","));
    let limits = Limits { memory: 4 * 1024 * 1024, ..Default::default() };
    let out = run("pm.response.json();", &post(200, "OK", &body), &limits);
    assert!(out.error.unwrap().message.contains("out of memory (limit 4 MB)"));
}

#[test]
fn event_streams_expose_their_events() {
    let mut input = post(200, "OK", "data: 1\n\n");
    if let Some(r) = &mut input.response {
        r.events = Some(vec![
            ScriptEvent { event: "progress".into(), data: "{\"pct\": 50}".into(), id: Some("1".into()) },
            ScriptEvent { event: "done".into(), data: "ok".into(), id: None },
        ]);
    }
    let out = run_ok(
        "pm.test('events', () => {
           pm.expect(pm.response.events.length).to.equal(2);
           pm.expect(JSON.parse(pm.response.events[0].data).pct).to.equal(50);
           pm.expect(pm.response.events[1].event).to.equal('done');
           pm.expect(pm.response.events[1].id).to.equal(null);
         });",
        &input,
    );
    assert!(out.tests.iter().all(|t| t.passed), "{:?}", out.tests);
    // Other responses have no events.
    let out =
        run_ok("pm.test('none', () => pm.expect(pm.response.events).to.equal(undefined));", &post(200, "OK", "{}"));
    assert!(out.tests.iter().all(|t| t.passed), "{:?}", out.tests);
}

#[test]
fn json_schema_assertion() {
    let schema = r#"{ type: 'object', required: ['id', 'name'], properties: { id: { type: 'integer' }, name: { type: 'string' } } }"#;
    let out = run_ok(
        &format!("pm.test('valid', () => pm.response.to.have.jsonSchema({schema}));"),
        &post(200, "OK", r#"{"id": 7, "name": "Rex"}"#),
    );
    assert!(out.tests[0].passed, "{:?}", out.tests);
    let out = run_ok(
        &format!("pm.test('invalid', () => pm.response.to.have.jsonSchema({schema}));"),
        &post(200, "OK", r#"{"id": "7"}"#),
    );
    let error = out.tests[0].error.clone().unwrap();
    assert!(!out.tests[0].passed);
    assert!(
        error.contains("body/id must be integer") && error.contains("must have required property 'name'"),
        "{error}"
    );
}
