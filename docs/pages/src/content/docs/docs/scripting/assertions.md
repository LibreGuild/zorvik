---
title: Assertions
description: Every pm.expect chain and assertion, and every pm.response.to assertion, with examples and failure messages.
sidebar:
  order: 3
---

`pm.expect` is a chai-style (BDD) assertion library built into the sandbox. It supports the chai words most collections use, and the Postman response assertions (`pm.response.to.…`). This page lists all of them.

An assertion that fails throws an `AssertionError`. Inside `pm.test` that fails the test and the script goes on; outside `pm.test` it stops the script.

```js
pm.test("user is valid", () => {
  const user = pm.response.json();
  pm.expect(user).to.be.an("object");
  pm.expect(user.id).to.be.a("number").and.above(0);
  pm.expect(user.email).to.match(/@/);
  pm.expect(user.roles).to.include("admin");
});
```

## pm.expect(value, message?)

```ts
pm.expect(value: any, message?: string): Assertion
```

`value` is the subject of the assertion. When `message` is given, it's put in front of the failure message:

```js
pm.expect(1, "status check").to.equal(2);
// AssertionError: status check: expected 1 to equal 2
```

The general assertion methods on this page also take an optional last `message` argument that does the same, for example `equal(200, "status check")`. The exceptions are `keys` and the [response assertions](#response-assertions), which don't take one.

## Language chains

These words only make an assertion read well; they don't change it. Chain them in any order:

`to`, `be`, `been`, `is`, `that`, `which`, `and`, `has`, `have`, `with`, `at`, `of`, `same`, `but`, `does`, `still`, `also`

```js
pm.expect(5).to.be.at.least(5).and.at.most(10);
```

## Flags

Flags change the assertions that follow them.

| Flag | Effect | Used by |
|---|---|---|
| `not` | Negates the assertion | All |
| `deep` | Compares by value instead of `===` | `equal`, `include`, `property`, `members`, `oneOf` |
| `nested` | Reads a dotted path like `a.b[1].c` | `property` |
| `own` | Only the object's own properties, not inherited ones | `property` |
| `ordered` | Members must be in the same order | `members` |
| `any` | At least one of the keys | `keys` |
| `all` | All of the keys (the default) | `keys` |
| `include`, `includes`, `contain`, `contains` | Used as a word before another assertion: allows a subset | `keys`, `members`, `oneOf` |

```js
pm.expect({ a: 1 }).to.not.have.property("b");
pm.expect({ a: { b: [1, { c: 3 }] } }).to.have.nested.property("a.b[1].c", 3);
pm.expect({ a: 1, b: 2 }).to.include.keys("a");
```

## Truthiness, existence and types

These are properties: write them without parentheses.

| Assertion | Passes when | Example |
|---|---|---|
| `ok` | The value is truthy | `pm.expect(body.active).to.be.ok` |
| `true` | The value is exactly `true` | `pm.expect(body.verified).to.be.true` |
| `false` | The value is exactly `false` | `pm.expect(body.deleted).to.be.false` |
| `null` | The value is `null` | `pm.expect(body.parent).to.be.null` |
| `undefined` | The value is `undefined` | `pm.expect(body.password).to.be.undefined` |
| `NaN` | The value is `NaN` | `pm.expect(Number("x")).to.be.NaN` |
| `finite` | The value is a number other than `Infinity`, `-Infinity` or `NaN` | `pm.expect(body.total).to.be.finite` |
| `exist`, `exists` | The value is neither `null` nor `undefined` | `pm.expect(body.id).to.exist` |
| `empty` | A string or array with length 0, a `Map` or `Set` with size 0, or an object without own enumerable keys | `pm.expect(body.errors).to.be.empty` |

`empty` fails with `.empty was passed non-string primitive …` for numbers, booleans, `null`, `undefined` and functions, even with `not`.

### a / an

| Assertion | Passes when |
|---|---|
| `a(type)`, `an(type)` | The value's type is `type` (case doesn't matter) |

The type names are: `string`, `number`, `boolean`, `undefined`, `null`, `object`, `array`, `function`, `bigint`, `symbol`, `date`, `regexp`, `error`, `map`, `set`, `promise`. Arrays are `array` (not `object`), and `null` is `null`.

```js
pm.expect("x").to.be.a("string");
pm.expect([]).to.be.an("array").that.is.empty;
pm.expect(new Date()).to.be.a("date");
```

The chain can go on after `a` and `an`: `pm.expect(x).to.be.an("array").with.lengthOf(2)`.

## Equality

| Assertion | Aliases | Passes when |
|---|---|---|
| `equal(value)` | `equals`, `eq` | The subject is `===` to `value`. With `deep`, compared by value like `eql`. |
| `eql(value)` | `eqls` | The subject equals `value` by value: same type, same keys and same values, recursively. Works for arrays, plain objects, `Date`, `RegExp`, `Map`, `Set` and errors (same name and message). |

```js
pm.expect(pm.response.code).to.equal(200);
pm.expect(body.tags).to.eql(["a", "b"]);
pm.expect({ a: 1 }).to.deep.equal({ a: 1 });
```

## Numbers and dates

| Assertion | Aliases | Passes when |
|---|---|---|
| `above(n)` | `gt`, `greaterThan` | subject `>` n |
| `least(n)` | `gte`, `greaterThanOrEqual` | subject `>=` n |
| `below(n)` | `lt`, `lessThan` | subject `<` n |
| `most(n)` | `lte`, `lessThanOrEqual` | subject `<=` n |
| `within(start, finish)` | | `start <= subject <= finish` (both ends included) |
| `closeTo(expected, delta)` | `approximately` | `abs(subject - expected) <= delta` |

`above`, `least`, `below` and `most` need a number or a `Date` as the subject; anything else fails with `expected … to be a number or a date`. After `length` or `lengthOf` they compare the length instead (see below).

```js
pm.expect(pm.response.responseTime).to.be.below(500);
pm.expect(body.price).to.be.within(1, 100);
pm.expect(body.ratio).to.be.closeTo(0.5, 0.01);
pm.expect(new Date(body.createdAt)).to.be.below(new Date());
```

## Strings

| Assertion | Aliases | Passes when |
|---|---|---|
| `include(text)` | `includes`, `contain`, `contains` | The string contains `text` |
| `string(text)` | | The subject is a string that contains `text` |
| `match(regexp)` | `matches` | `regexp` matches the string |
| `lengthOf(n)` / `length(n)` | | The string's length is `n` |

```js
pm.expect(pm.response.headers.get("content-type")).to.include("application/json");
pm.expect(body.id).to.match(/^[0-9a-f-]{36}$/);
```

## Arrays, sets, maps and objects

### include

`include(value)` (aliases `includes`, `contain`, `contains`) checks, depending on the subject:

| Subject | Passes when |
|---|---|
| String | It contains `value` as text |
| Array | One item is `=== value` (with `deep`: equal by value) |
| `Set` | It has `value` (with `deep`: an item equal by value) |
| `Map` | One of its values is `value` (with `deep`: equal by value) |
| Object | Every key of `value` is in the subject with the same value (with `deep`: equal by value) |

```js
pm.expect([1, 2, 3]).to.include(2);
pm.expect([{ id: 1 }, { id: 2 }]).to.deep.include({ id: 2 });
pm.expect({ id: 1, name: "Ada", role: "admin" }).to.include({ role: "admin" });
```

### length and lengthOf

| Assertion | Passes when |
|---|---|
| `lengthOf(n)`, `length(n)` | The subject's `length` (or `size` for `Map` and `Set`) is `n` |
| `.length.above(n)`, `.lengthOf.below(n)`, … | `above`, `least`, `below`, `most` and `within` after `length` or `lengthOf` compare the length |

```js
pm.expect(body.items).to.have.lengthOf(3);
pm.expect(body.items).to.have.length.above(0);
pm.expect(body.items).to.have.lengthOf.within(1, 50);
```

### members

`members(list)` compares arrays as sets of items.

| Form | Passes when |
|---|---|
| `members(list)` | Same items as `list`, in any order (and the same count) |
| `include.members(list)` | Every item of `list` is in the subject |
| `ordered.members(list)` | Same items in the same order |
| `include.ordered.members(list)` | The subject starts with the items of `list`, in order |
| `deep.members(list)` | Items compared by value |

```js
pm.expect([1, 2, 3]).to.have.members([3, 2, 1]);
pm.expect([1, 2, 3]).to.include.members([2]);
pm.expect(body.users).to.have.deep.members([{ id: 1 }, { id: 2 }]);
```

### keys

`keys(...names)` (alias `key`) takes names as arguments, an array, or an object (its keys are used). It doesn't take a message argument.

| Form | Passes when |
|---|---|
| `keys("a", "b")` or `all.keys(…)` | The subject has exactly these keys, no more and no fewer |
| `include.keys(…)`, `contain.all.keys(…)` | The subject has at least these keys |
| `any.keys(…)` | The subject has at least one of these keys |

For objects the keys are its own enumerable keys; for `Map` and `Set` they are the map's keys or the set's values.

```js
pm.expect(body).to.have.all.keys("id", "name", "email");
pm.expect(body).to.include.keys("id");
pm.expect(body).to.have.any.keys("email", "phone");
```

### oneOf

| Form | Passes when |
|---|---|
| `oneOf(list)` | The subject is `===` to an item of `list` (with `deep`: equal by value) |
| `include.oneOf(list)` | The subject (a string or an array) contains one of the items |

```js
pm.expect(body.status).to.be.oneOf(["active", "pending"]);
```

## Properties

`property(name, value?)` checks that the subject has a property, and optionally its value.

| Form | Passes when |
|---|---|
| `property(name)` | The subject has the property (own or inherited) |
| `property(name, value)` | …and its value is `=== value` |
| `deep.property(name, value)` | …and its value equals `value` by value |
| `own.property(name)`, `ownProperty(name)`, `haveOwnProperty(name)` | The property is the subject's own |
| `nested.property("a.b[0].c")` | The dotted path exists (`[n]` for array indexes) |
| `not.property(name)` | The property doesn't exist |
| `not.property(name, value)` | The property doesn't have that value (it may exist) |

After `property`, the rest of the chain is about the **property's value**:

```js
pm.expect(body).to.have.property("user").that.has.property("id");
pm.expect(body).to.have.property("items").with.lengthOf(2);
pm.expect(body).to.have.nested.property("user.address.city", "Paris");
pm.expect(body).to.have.deep.property("tags", ["a", "b"]);
```

## Functions and errors

| Assertion | Aliases | Passes when |
|---|---|---|
| `throw()` | `throws`, `Throw` | Calling the subject (a function) throws |
| `throw(ErrorType)` | | …an error that is an instance of `ErrorType` |
| `throw("text")`, `throw(/regexp/)` | | …an error whose message contains `text` or matches `regexp` |
| `throw(ErrorType, "text" or /regexp/)` | | Both |
| `satisfy(fn)` | `satisfies` | `fn(subject)` returns a truthy value |
| `instanceOf(Type)` | `instanceof` | `subject instanceof Type` |

After `throw`, the rest of the chain is about the thrown error.

```js
pm.expect(() => JSON.parse("{")).to.throw(SyntaxError);
pm.expect(() => pm.response.json()).to.not.throw();
pm.expect(body.count).to.satisfy((n) => n % 2 === 0);
pm.expect(new Date(body.createdAt)).to.be.instanceOf(Date);
```

## Response assertions

`pm.response.to` (and `pm.expect(pm.response).to`) adds Postman's response assertions. All language chains and `not` work; the general assertions above work too, on the response object.

### Status

| Assertion | Passes when the status is |
|---|---|
| `status(code)` | `code`, for example `pm.response.to.have.status(201)` |
| `status(reason)` | A string compares the reason phrase: `pm.response.to.have.status("Created")` |
| `ok` | 200 |
| `accepted` | 202 |
| `withoutContent` | 204 |
| `badRequest` | 400 |
| `unauthorized`, `unauthorised` | 401 |
| `forbidden` | 403 |
| `notFound` | 404 |
| `notAcceptable` | 406 |
| `rateLimited` | 429 |
| `info` | 1XX |
| `success` | 2XX |
| `redirection` | 3XX |
| `clientError` | 4XX |
| `serverError` | 5XX |
| `error` | 4XX or 5XX |

On a response, `ok` means status 200 (not "truthy").

```js
pm.response.to.have.status(200);
pm.response.to.be.success;
pm.response.to.not.be.error;
pm.expect(pm.response).to.have.status(201);
```

### Headers and body

| Assertion | Passes when |
|---|---|
| `header(name)` | The response has the header (name without case) |
| `header(name, value)` | …and its first value is exactly `value` |
| `withBody` | The body is not empty |
| `body()` | The body is not empty |
| `body("text")` | The body is exactly `text` |
| `body(/regexp/)` | `regexp` matches the body |
| `body(object)` | The body is JSON equal to `object` by value |
| `json` | The body is valid JSON |
| `jsonBody()` | The body is valid JSON |
| `jsonBody(object)` | The body is JSON equal to `object` by value |
| `jsonBody(path)` | The JSON body has the dotted path, for example `data.items[0].id` |
| `jsonBody(path, value)` | …and the value there equals `value` by value |
| `jsonSchema(schema)` | Not supported: throws `pm.response.to.have.jsonSchema is not supported in Zorvik` |

```js
pm.response.to.have.header("Content-Type");
pm.response.to.have.header("Cache-Control", "no-store");
pm.response.to.be.json;
pm.response.to.have.jsonBody("data.items[0].id", 1);
pm.response.to.have.body(/"status":\s*"ok"/);
```

## Failure messages

Failure messages follow chai's style, so they read like the assertion. Some examples:

| Assertion | Message |
|---|---|
| `pm.expect(404).to.equal(200)` | `expected 404 to equal 200` |
| `pm.expect('a').to.be.a('number')` | `expected 'a' to be a number` |
| `pm.expect({ a: 1 }).to.eql({ a: 2 })` | `expected { a: 1 } to deeply equal { a: 2 }` |
| `pm.expect(5).to.not.be.below(10)` | `expected 5 to be at least 10` |
| `pm.expect([1, 2]).to.have.lengthOf(3)` | `expected [ 1, 2 ] to have a length of 3 but got 2` |
| `pm.expect({ a: 1 }).to.have.property('a', 2)` | `expected { a: 1 } to have property 'a' of 2, but got 1` |
| `pm.expect('x').to.be.oneOf(['a', 'b'])` | `expected 'x' to be one of [ 'a', 'b' ]` |
| `pm.expect({ a: 1 }).to.have.keys('a', 'b')` | `expected { a: 1 } to have keys 'a', and 'b'` |
| `pm.expect(5).to.be.within(1, 3)` | `expected 5 to be within 1..3` |
| `pm.expect(() => {}).to.throw()` | `expected [Function] to throw an error` |
| `pm.response.to.have.status(200)` | `expected response to have status code 200 but got 404` |
| `pm.response.to.be.ok` | `expected response code to be 200 but found 404` |
| `pm.response.to.be.success` | `expected response code to be 2XX but found 404` |
| `pm.response.to.have.header('X-Nope')` | `expected response to have header with key 'X-Nope'` |

Long values are shortened: a large object is shown as `{ Object (id, name, ...) }` and a long array as `[ Array(120) ]`.

## Differences from chai

The library covers the common part of chai. Keep these differences in mind:

:::caution[Unknown words pass silently]
A chai word Zorvik doesn't have, used as a property, reads as `undefined` and **asserts nothing**, so the test passes. For example `pm.expect(obj).to.be.frozen` always passes. Stick to the words on this page. An unknown word called as a function (`.to.be.frozenish()`) throws a `TypeError` instead.
:::

Not supported:

- `nested` and `own` have no effect on `include`: `.nested.include({ "a.b": 1 })` fails, and `.own.include(…)` also finds inherited properties. Use `nested.property` and `own.property` instead.
- `sealed`, `frozen`, `extensible`, `arguments`, `itself`, `respondTo`, `increase`, `decrease`, `change`, `by`, `fail` and `pm.expect.fail`.
- `chai.assert` and `chai.should` styles: only `pm.expect` exists.
- `jsonSchema` (see [Script examples](../examples/#schema-like-checks) for checks you can write instead, and [OpenAPI contract checks](../../testing/openapi-contract-checks/) for automatic ones).

Other details:

- `lengthOf(n)` compares with `==`, so `lengthOf("2")` passes for a length of 2.
- `include(object)` on an object also finds inherited properties.
- `closeTo` and `within` don't check the subject's type, unlike `above`, `least`, `below` and `most`.
