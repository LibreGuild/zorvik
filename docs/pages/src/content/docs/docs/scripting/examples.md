---
title: Script examples
description: Copy-ready recipes for status and JSON checks, tokens, chained ids, headers, time limits, schema-like checks, event streams and run control.
sidebar:
  order: 4
---

Each recipe says where the code goes: a **pre-request** or **post-response** script of a request, a folder or the workspace. See [Scripts overview](../overview/#where-scripts-live) for where to edit them.

## Check the status and JSON fields

```js title="Post-response script"
pm.test("Status is 200", () => {
  pm.response.to.have.status(200);
});

pm.test("Returns the user", () => {
  const user = pm.response.json();
  pm.expect(user).to.have.property("id").that.is.a("number");
  pm.expect(user.name).to.be.a("string").and.not.be.empty;
  pm.expect(user.email).to.match(/^[^@\s]+@[^@\s]+$/);
  pm.expect(user.roles).to.include("admin");
});

pm.test("City is Paris", () => {
  pm.response.to.have.jsonBody("address.city", "Paris");
});
```

Call `pm.response.json()` inside the test when the body might not be JSON: if it isn't, only that test fails. Called at the top level of the script, it stops the whole script.

To accept several statuses:

```js
pm.test("Created or already there", () => {
  pm.expect(pm.response.code).to.be.oneOf([200, 201]);
});
```

## Save a token for the next requests

Put a login request first, and save the token it returns:

```js title="Post-response script of 'Log in'"
pm.test("Logged in", () => pm.response.to.have.status(200));

const { access_token } = pm.response.json();
pm.environment.set("token", access_token);
```

Then use `{{token}}` in the auth of the folder or workspace, so every request inside gets it. In the folder's **Auth** tab choose **Bearer** and enter `{{token}}`, or in the file:

```yaml title="requests/Orders/_folder.yaml"
name: Orders
auth:
  type: bearer
  token: "{{token}}"
```

In the app, the token is kept on this computer for the active environment, never in the environment file. With `zorvik run` it lasts until the run ends. Declare `token` as a secret variable in the environment: if its value ever appears in a URL, run reports and history show `{{token}}` instead.

:::tip
A pre-request script can also log in by itself with [`pm.sendRequest`](../pm-reference/#pmsendrequest), for APIs whose login isn't OAuth:

```js title="Pre-request script of the folder"
if (!pm.environment.get("token")) {
  const res = await pm.sendRequest({
    url: pm.variables.replaceIn("{{baseUrl}}/login"),
    method: "POST",
    header: { "Content-Type": "application/json" },
    body: { mode: "raw", raw: JSON.stringify({ user: pm.environment.get("user"), password: pm.environment.get("password") }) },
  });
  pm.environment.set("token", res.json().token);
}
```

For OAuth 2.0, the **OAuth 2.0** auth type fetches and refreshes tokens on its own. See [Auth](../../requests/auth/).
:::

## Chain ids from one request to the next

```js title="Post-response script of 'Create order'"
pm.test("Order created", () => pm.response.to.have.status(201));
pm.collectionVariables.set("orderId", pm.response.json().id);
```

Later requests use it anywhere a variable works:

```text
GET    {{base}}/orders/{{orderId}}
DELETE {{base}}/orders/{{orderId}}
```

Which scope to use:

- `pm.collectionVariables` or `pm.environment`: the value is still there when you send the next request by hand in the app.
- `pm.variables`: the value lasts for one send in the app, or for the whole collection run. Use it for values that only matter inside a run and shouldn't be kept.

To pass a list, store it as JSON and parse it when reading:

```js title="Post-response script of 'List items'"
const ids = pm.response.json().items.map((item) => item.id);
pm.collectionVariables.set("itemIds", ids);   // stored as "[1,2,3]"
```

```js title="Pre-request script of 'Delete first item'"
const [first] = JSON.parse(pm.collectionVariables.get("itemIds") || "[]");
pm.variables.set("itemId", first);
```

## Check headers

Header names are compared without case.

```js title="Post-response script"
pm.test("JSON content type", () => {
  pm.response.to.have.header("Content-Type");
  pm.expect(pm.response.headers.get("content-type")).to.include("application/json");
});

pm.test("Not cached", () => {
  pm.response.to.have.header("Cache-Control", "no-store");
});

pm.test("CORS allows our app", () => {
  pm.expect(pm.response.headers.get("Access-Control-Allow-Origin")).to.be.oneOf(["*", "https://app.example.com"]);
});

pm.test("Has a request id", () => {
  pm.expect(pm.response.headers.has("X-Request-Id")).to.be.true;
});
```

`headers.get` returns the first value of a header. To see every `Set-Cookie` header:

```js
const cookies = pm.response.headers.filter((h) => h.key.toLowerCase() === "set-cookie").map((h) => h.value);
```

## Check response times

```js title="Post-response script"
pm.test("Answers within 500 ms", () => {
  pm.expect(pm.response.responseTime).to.be.below(500);
});
```

`responseTime` is the total time in milliseconds. Zorvik opens a new connection for every request, so it includes DNS, connecting and TLS. For an event stream in a run it's the time spent reading the stream.

To check every request, put the test in a folder or workspace post-response script, with the limit in a variable:

```js title="Workspace post-response script"
const limit = Number(pm.variables.get("maxResponseMs") || 1000);
pm.test(`Faster than ${limit} ms`, () => {
  pm.expect(pm.response.responseTime).to.be.below(limit);
});
```

:::caution
In a collection run, a request **without tests** fails when its status is 400 or more, but a request **with tests** passes when all its tests pass. A test added by a workspace or folder script counts too, so a `404` passes if its only test is the time check. Add a status test where the status matters.
:::

## Schema checks

The shortest way is the `jsonSchema` assertion:

```js title="Post-response script"
pm.test("Body matches the schema", () => {
  pm.response.to.have.jsonSchema({
    type: "object",
    required: ["id", "email"],
    properties: { id: { type: "integer" }, email: { type: "string", format: "email" } },
  });
});
```

For more control, use the built-in [Ajv](../sandbox/#libraries) yourself:

```js title="Post-response script"
const Ajv = require("ajv");
const ajv = new Ajv({ allErrors: true });
const schema = {
  type: "object",
  required: ["id", "email"],
  properties: {
    id: { type: "integer" },
    email: { type: "string", format: "email" },
    createdAt: { type: "string", format: "date-time" },
  },
};

pm.test("Body matches the schema", () => {
  const valid = ajv.validate(schema, pm.response.json());
  pm.expect(valid, ajv.errorsText()).to.be.true;
});
```

A failure names every problem: `data must have required property 'email', data/id must be integer`. `tv4.validate(data, schema)` works too, for older draft-04 schemas.

Without a schema, checks like these cover most needs:

```js title="Post-response script"
/** Check that `value` has each key of `shape` with the given type ("string", "number", "array", "object", "boolean", "null"). */
function checkShape(value, shape, path) {
  for (const [key, type] of Object.entries(shape)) {
    pm.expect(value, path).to.have.property(key);
    pm.expect(value[key], `${path}.${key}`).to.be.a(type);
  }
}

pm.test("Users have the documented shape", () => {
  const users = pm.response.json();
  pm.expect(users).to.be.an("array").that.is.not.empty;
  users.forEach((user, i) => {
    checkShape(user, { id: "number", name: "string", email: "string", roles: "array" }, `$[${i}]`);
    pm.expect(user.status, `$[${i}].status`).to.be.oneOf(["active", "invited", "disabled"]);
  });
});

pm.test("No unexpected fields", () => {
  pm.expect(pm.response.json()[0]).to.have.all.keys("id", "name", "email", "roles", "status");
});
```

The message argument (`$[0].email`) tells you which item failed: `$[0].email: expected null to be a string`.

For a nullable field:

```js
pm.expect(user.deletedAt === null || typeof user.deletedAt === "string", "deletedAt").to.be.true;
```

If the API has an OpenAPI document, import it: Zorvik then checks every response against the documented schemas automatically. See [OpenAPI contract checks](../../testing/openapi-contract-checks/).

## Test Server-Sent Events

In a collection run, an SSE request is read until the stop condition in its **Settings** tab (for example, until an event named `done`), and its post-response script gets the events in `pm.response.events`.

```yaml title="requests/Jobs/Watch job.yaml"
name: Watch job
kind: sse
url: "{{base}}/jobs/{{jobId}}/events"
settings:
  stream:
    event: done
    maxEvents: 100
    timeoutMs: 30000
scripts:
  postResponse: |
    const events = pm.response.events;

    pm.test("The job finished", () => {
      pm.expect(events.length).to.be.above(0);
      pm.expect(events[events.length - 1].event).to.equal("done");
    });

    pm.test("Progress never goes backwards", () => {
      const pct = events.filter((e) => e.event === "progress").map((e) => JSON.parse(e.data).pct);
      pm.expect(pct).to.eql([...pct].sort((a, b) => a - b));
    });
```

Events without an `event:` line have the name `message`. See [Repeat until and event streams](../../testing/repeat-and-streams/#event-streams-in-runs).

## Add headers before sending

```js title="Pre-request script"
pm.request.headers.upsert({ key: "X-Request-Id", value: pm.variables.replaceIn("{{$guid}}") });
pm.request.headers.upsert({ key: "X-Sent-At", value: new Date().toISOString() });
```

Basic credentials by hand (the **Basic** auth type does this for you):

```js title="Pre-request script"
const user = pm.variables.get("user");
const password = pm.variables.get("password");
pm.request.headers.upsert({ key: "Authorization", value: "Basic " + btoa(`${user}:${password}`) });
```

`btoa` only encodes Latin-1 text; for any text use `CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse(text))`.

Sign a request with HMAC-SHA256, as many APIs ask, with the built-in [crypto-js](../sandbox/#libraries):

```js title="Pre-request script"
const timestamp = Date.now().toString();
const payload = [pm.request.method, pm.request.url.getPathWithQuery(), timestamp, pm.request.body.raw || ""].join("\n");
const signature = CryptoJS.HmacSHA256(payload, pm.environment.get("apiSecret")).toString(CryptoJS.enc.Base64);
pm.request.headers.upsert({ key: "X-Timestamp", value: timestamp });
pm.request.headers.upsert({ key: "X-Signature", value: signature });
```

## Change the body before sending

```js title="Pre-request script"
const body = JSON.parse(pm.request.body.raw || "{}");
body.requestedAt = Date.now();
body.reference = pm.variables.replaceIn("order-{{$randomInt}}");
pm.request.body.raw = JSON.stringify(body);
```

This works for JSON, Text and XML bodies. The change applies to this send only.

## Data-driven tests

With a data file such as:

```csv title="logins.csv"
username,password,expectedStatus
ada,correct-horse,200
bob,wrong,401
```

the request can use `{{username}}` and `{{password}}`, and the test can read the expected result:

```js title="Post-response script of 'Log in'"
const expected = Number(pm.iterationData.get("expectedStatus"));
pm.test(`${pm.iterationData.get("username")} gets ${expected}`, () => {
  pm.response.to.have.status(expected);
});
```

CSV values are strings, hence `Number(…)`. See [Data files](../../testing/data-files/).

## Control the order of a run

Run a request again until the server is ready, up to 3 times (the ["repeat until" setting](../../testing/repeat-and-streams/) is often simpler):

```js title="Post-response script of 'Check status'"
const tries = Number(pm.variables.get("tries") || 0) + 1;
if (pm.response.code === 503 && tries < 3) {
  pm.variables.set("tries", tries);
  pm.execution.setNextRequest(pm.info.requestName);
} else {
  pm.variables.unset("tries");
}
```

Choose the next request from the response:

```js title="Post-response script of 'Get account'"
pm.execution.setNextRequest(pm.response.json().isAdmin ? "Admin dashboard" : "User home");
```

End the iteration early:

```js title="Post-response script"
if (pm.response.code === 404) {
  pm.execution.setNextRequest(null);   // skip the rest of this iteration
}
```

`setNextRequest` only has an effect in collection runs. See [Collection runner](../../testing/collection-runner/#changing-the-order-with-setnextrequest).

## Skip tests on some environments

```js title="Post-response script"
const test = pm.environment.name === "Production" ? pm.test.skip : pm.test;

test("Creates a test order", () => {
  pm.response.to.have.status(201);
});
```

Skipped tests show as skipped and don't fail anything.

## Wait for a job to finish

To send a request again until a job is done, don't loop in a script (there are no timers to wait with). Turn on **Repeat until** in the request's **Settings** tab with a condition such as:

```js
pm.response.json().status === "done"
```

See [Repeat until and event streams](../../testing/repeat-and-streams/).

## Debug a script

```js
console.log("sending", pm.request.method, pm.request.url.toString());
console.log("body starts with", pm.response.text().slice(0, 300));
console.log("all variables", pm.variables.toObject());
```

Output shows in the response's **Console** tab, in expanded results of the Runner, and in `results[].console` of `zorvik run --json`.

:::caution
Scripts can read secret variable values. Anything you log appears in the console and in exported run reports, so don't log tokens or passwords.
:::
