// The script API: a Postman-compatible `pm` subset, a chai-style `pm.expect`
// and `console`. Evaluated in every fresh context before the user's script;
// it returns `setup(inputJson, responseBody)`, which installs the globals and
// returns `finish()`, which reports what the script did as JSON.
(function () {
  'use strict';

  var G = globalThis;
  var jsonStringify = JSON.stringify;
  var jsonParse = JSON.parse;
  var hasOwn = Object.prototype.hasOwnProperty;
  var objectToString = Object.prototype.toString;

  var MAX_CONSOLE = 1000;
  var MAX_TEXT = 10 * 1024;
  var MAX_TESTS = 10000;
  var MAX_RENDER = 16 * 1024 * 1024;
  var MAX_VISUALIZATION = 5 * 1024 * 1024;

  // ---- helpers ----------------------------------------------------------------

  function clip(s) {
    return s.length > MAX_TEXT ? s.slice(0, MAX_TEXT) + '… (truncated)' : s;
  }

  function own(o, k) {
    return o !== null && o !== undefined && hasOwn.call(o, k);
  }

  function copy(o) {
    var r = {};
    for (var k in o) if (own(o, k)) r[k] = o[k];
    return r;
  }

  /** `array`, `null`, `date`, `regexp`, `map`, … or `typeof` for primitives. */
  function typeOf(v) {
    if (v === null) return 'null';
    if (Array.isArray(v)) return 'array';
    var t = typeof v;
    if (t !== 'object') return t;
    return objectToString.call(v).slice(8, -1).toLowerCase();
  }

  function unsupported(name) {
    return function () {
      throw new Error(name + ' is not supported in Zorvik');
    };
  }

  /** JSON that survives cycles, BigInt and functions. */
  function safeJson(v) {
    var seen = new Set();
    try {
      return jsonStringify(v, function (key, value) {
        if (typeof value === 'bigint') return value.toString();
        if (typeof value === 'function') return '[Function' + (value.name ? ': ' + value.name : '') + ']';
        if (value !== null && typeof value === 'object') {
          if (seen.has(value)) return '[Circular]';
          seen.add(value);
        }
        return value;
      });
    } catch (e) {
      return String(v);
    }
  }

  /** Variable values are strings outside scripts: objects become JSON. */
  function stored(v) {
    if (typeof v === 'string') return v;
    if (v === undefined) return '';
    if (v !== null && typeof v === 'object') return safeJson(v);
    return String(v);
  }

  function show(v) {
    if (typeof v === 'string') return v;
    if (v instanceof Error) return v.name + ': ' + v.message;
    if (typeof v === 'function') return '[Function' + (v.name ? ': ' + v.name : '') + ']';
    if (v !== null && typeof v === 'object') {
      var s = safeJson(v);
      return s === undefined ? String(v) : s;
    }
    return String(v);
  }

  function errorText(e) {
    if (e instanceof AssertionError) return String(e.message);
    if (e instanceof Error) return e.name + ': ' + e.message;
    return show(e);
  }

  // ---- console ----------------------------------------------------------------

  var logs = [];
  var dropped = 0;

  /** `console.log('%s is %d', a, b)` placeholders, then the other arguments. */
  function format(args) {
    var rest = Array.prototype.slice.call(args);
    var parts = [];
    if (typeof rest[0] === 'string' && rest.length > 1 && rest[0].indexOf('%') !== -1) {
      parts.push(
        rest.shift().replace(/%[sdifjoO%]/g, function (m) {
          if (m === '%%') return '%';
          if (!rest.length) return m;
          var v = rest.shift();
          if (m === '%d' || m === '%i') return String(m === '%d' ? Number(v) : parseInt(v, 10));
          if (m === '%f') return String(parseFloat(v));
          if (m === '%s') return show(v);
          return safeJson(v);
        })
      );
    }
    for (var i = 0; i < rest.length; i++) parts.push(show(rest[i]));
    return parts.join(' ');
  }

  function logger(level) {
    return function () {
      if (logs.length >= MAX_CONSOLE) {
        dropped++;
        return;
      }
      logs.push({ level: level, message: clip(format(arguments)) });
    };
  }

  var consoleApi = {
    log: logger('log'),
    info: logger('info'),
    warn: logger('warn'),
    error: logger('error'),
    debug: logger('debug'),
    dir: logger('log'),
    trace: logger('debug'),
    clear: function () {},
  };

  // ---- variables ----------------------------------------------------------------

  function uuid() {
    var hex = '0123456789abcdef';
    var s = '';
    for (var i = 0; i < 36; i++) {
      if (i === 8 || i === 13 || i === 18 || i === 23) s += '-';
      else if (i === 14) s += '4';
      else if (i === 19) s += hex[8 + ((Math.random() * 4) | 0)];
      else s += hex[(Math.random() * 16) | 0];
    }
    return s;
  }

  /** The app's dynamic variables (`host.dynamic`, the same as requests use); set by `setup`. */
  var hostDynamic = null;

  /** Postman dynamic variables without the app (the script engine's own tests). */
  function dynamic(name) {
    switch (name) {
      case 'guid':
      case 'uuid':
      case 'randomUUID':
        return uuid();
      case 'timestamp':
        return String(Math.floor(Date.now() / 1000));
      case 'timestampMs':
        return String(Date.now());
      case 'isoTimestamp':
        return new Date().toISOString();
      case 'randomInt':
        return String(Math.floor(Math.random() * 1001));
      case 'randomBoolean':
        return String(Math.random() < 0.5);
      case 'randomAlphaNumeric':
        return 'abcdefghijklmnopqrstuvwxyz0123456789'.charAt(Math.floor(Math.random() * 36));
      case 'randomEmail':
        return 'user' + (1000 + Math.floor(Math.random() * 9000)) + '@example.com';
    }
    return undefined;
  }

  /** Replace `{{name}}` using `lookup`, nested up to 10 levels. */
  function render(template, lookup) {
    if (typeof template !== 'string') return template;
    var out = template;
    for (var depth = 0; depth < 10 && out.indexOf('{{') !== -1; depth++) {
      var changed = false;
      out = out.replace(/\{\{([^{}\n]+)\}\}/g, function (m, raw) {
        var name = raw.trim();
        var v = lookup(name);
        if (v === undefined && name.charAt(0) === '$') {
          v = hostDynamic ? hostDynamic(name) : undefined;
          if (v === undefined || v === null) v = dynamic(name.slice(1));
        }
        if (v === undefined) return m;
        changed = true;
        return stored(v);
      });
      if (!changed || out.length > MAX_RENDER) break;
    }
    return out;
  }

  // ---- assertions (chai BDD subset) ---------------------------------------------

  class AssertionError extends Error {
    constructor(message) {
      super(message);
      this.name = 'AssertionError';
    }
  }

  function isIdentifier(k) {
    return /^[A-Za-z_$][\w$]*$/.test(k);
  }

  /** chai-like display: `'text'`, `[ 1, 2 ]`, `{ a: 1 }`. */
  function inspect(v, depth) {
    depth = depth || 0;
    var t = typeOf(v);
    switch (t) {
      case 'string':
        return "'" + (v.length > 200 ? v.slice(0, 200) + '…' : v) + "'";
      case 'number':
        return v === 0 && 1 / v < 0 ? '-0' : String(v);
      case 'bigint':
        return String(v) + 'n';
      case 'symbol':
        return v.toString();
      case 'function':
        return '[Function' + (v.name ? ' ' + v.name : '') + ']';
      case 'regexp':
        return String(v);
      case 'date':
        return isNaN(v.getTime()) ? 'Invalid Date' : v.toISOString();
      case 'error':
        return v.name + ': ' + v.message;
      case 'array': {
        if (!v.length) return '[]';
        if (depth > 2) return '[ Array(' + v.length + ') ]';
        var items = [];
        for (var i = 0; i < v.length && i < 20; i++) items.push(inspect(v[i], depth + 1));
        if (v.length > 20) items.push('… ' + (v.length - 20) + ' more');
        return '[ ' + items.join(', ') + ' ]';
      }
      case 'map':
      case 'set':
        return (t === 'map' ? 'Map' : 'Set') + '(' + v.size + ')';
    }
    if (v === null || v === undefined || typeof v !== 'object') return String(v);
    var keys = Object.keys(v);
    if (!keys.length) return '{}';
    if (depth > 2) return '{ Object (' + keys.slice(0, 2).join(', ') + (keys.length > 2 ? ', ...' : '') + ') }';
    var props = [];
    for (var j = 0; j < keys.length && j < 20; j++) {
      props.push((isIdentifier(keys[j]) ? keys[j] : inspect(keys[j])) + ': ' + inspect(v[keys[j]], depth + 1));
    }
    if (keys.length > 20) props.push('…');
    return '{ ' + props.join(', ') + ' }';
  }

  /** `inspect`, shortened like chai's objDisplay when long. */
  function display(v) {
    var s = inspect(v);
    if (s.length < 40) return s;
    var t = typeOf(v);
    if (t === 'function') return v.name ? '[Function: ' + v.name + ']' : '[Function]';
    if (t === 'array') return '[ Array(' + v.length + ') ]';
    if (t === 'object') {
      var keys = Object.keys(v);
      return '{ Object (' + keys.slice(0, 2).join(', ') + (keys.length > 2 ? ', ...' : '') + ') }';
    }
    return s;
  }

  function enumerableKeys(o) {
    var keys = [];
    for (var k in o) keys.push(k);
    return keys.sort();
  }

  function deepEqual(a, b, memo) {
    if (a === b) return true;
    if (a !== a && b !== b) return true;
    var ta = typeOf(a);
    if (ta !== typeOf(b) || a === null || b === null || typeof a !== 'object') return false;
    memo = memo || [];
    for (var m = 0; m < memo.length; m++) if (memo[m][0] === a && memo[m][1] === b) return true;
    memo.push([a, b]);
    var i;
    switch (ta) {
      case 'date':
        return a.getTime() === b.getTime();
      case 'regexp':
        return String(a) === String(b);
      case 'error':
        return a.name === b.name && a.message === b.message;
      case 'string':
      case 'number':
      case 'boolean':
        return a.valueOf() === b.valueOf();
      case 'array':
        if (a.length !== b.length) return false;
        for (i = 0; i < a.length; i++) if (!deepEqual(a[i], b[i], memo)) return false;
        return true;
      case 'set': {
        if (a.size !== b.size) return false;
        var bs = Array.from(b);
        return Array.from(a).every(function (x) {
          return bs.some(function (y) {
            return deepEqual(x, y, memo);
          });
        });
      }
      case 'map':
        if (a.size !== b.size) return false;
        return Array.from(a).every(function (e) {
          return b.has(e[0]) && deepEqual(e[1], b.get(e[0]), memo);
        });
    }
    var ka = enumerableKeys(a);
    var kb = enumerableKeys(b);
    if (ka.length !== kb.length) return false;
    for (i = 0; i < ka.length; i++) if (ka[i] !== kb[i]) return false;
    for (i = 0; i < ka.length; i++) if (!deepEqual(a[ka[i]], b[ka[i]], memo)) return false;
    return true;
  }

  function Assertion(obj, message) {
    Object.defineProperty(this, '__flags', { value: { object: obj, message: message } });
  }

  function flag(a, key, value) {
    if (arguments.length === 3) a.__flags[key] = value;
    return a.__flags[key];
  }

  Assertion.prototype.assert = function (ok, message, negatedMessage) {
    var negate = flag(this, 'negate');
    if (negate ? ok : !ok) {
      var custom = flag(this, 'message');
      var text = negate ? negatedMessage : message;
      throw new AssertionError(custom ? custom + ': ' + text : text);
    }
  };

  function define(proto, name, getter) {
    Object.defineProperty(proto, name, { get: getter, configurable: true });
  }

  function method(proto, name, fn) {
    Object.defineProperty(proto, name, { value: fn, writable: true, configurable: true });
  }

  /** `expect(x).to.include.members(…)` and `expect(x).to.include(y)`: a getter returning a callable. */
  function chainable(name, fn, chain) {
    define(Assertion.prototype, name, function () {
      var self = this;
      if (chain) chain.call(self);
      var call = function () {
        var r = fn.apply(self, arguments);
        return r === undefined ? self : r;
      };
      Object.setPrototypeOf(call, self);
      return call;
    });
  }

  function assertion(names, fn) {
    names.forEach(function (name) {
      method(Assertion.prototype, name, function () {
        var r = fn.apply(this, arguments);
        return r === undefined ? this : r;
      });
    });
  }

  function property(name, fn) {
    define(Assertion.prototype, name, function () {
      fn.call(this);
      return this;
    });
  }

  [
    'to', 'be', 'been', 'is', 'that', 'which', 'and', 'has', 'have', 'with', 'at', 'of', 'same', 'but', 'does', 'still',
    'also',
  ].forEach(function (word) {
    define(Assertion.prototype, word, function () {
      return this;
    });
  });

  function setFlag(name, key, value, clear) {
    define(Assertion.prototype, name, function () {
      flag(this, key, value);
      if (clear) flag(this, clear, false);
      return this;
    });
  }
  setFlag('not', 'negate', true);
  setFlag('deep', 'deep', true);
  setFlag('nested', 'nested', true);
  setFlag('own', 'own', true);
  setFlag('ordered', 'ordered', true);
  setFlag('any', 'any', true, 'all');
  setFlag('all', 'all', true, 'any');

  function withMessage(a, message) {
    if (message !== undefined) flag(a, 'message', message);
  }

  property('ok', function () {
    var t = display(flag(this, 'object'));
    this.assert(flag(this, 'object'), 'expected ' + t + ' to be truthy', 'expected ' + t + ' to be falsy');
  });
  property('true', function () {
    var t = display(flag(this, 'object'));
    this.assert(flag(this, 'object') === true, 'expected ' + t + ' to be true', 'expected ' + t + ' to be false');
  });
  property('false', function () {
    var t = display(flag(this, 'object'));
    this.assert(flag(this, 'object') === false, 'expected ' + t + ' to be false', 'expected ' + t + ' to be true');
  });
  property('null', function () {
    var t = display(flag(this, 'object'));
    this.assert(flag(this, 'object') === null, 'expected ' + t + ' to be null', 'expected ' + t + ' not to be null');
  });
  property('undefined', function () {
    var t = display(flag(this, 'object'));
    this.assert(
      flag(this, 'object') === undefined,
      'expected ' + t + ' to be undefined',
      'expected ' + t + ' not to be undefined'
    );
  });
  property('NaN', function () {
    var o = flag(this, 'object');
    var t = display(o);
    this.assert(typeof o === 'number' && o !== o, 'expected ' + t + ' to be NaN', 'expected ' + t + ' not to be NaN');
  });
  property('finite', function () {
    var o = flag(this, 'object');
    var t = display(o);
    this.assert(
      typeof o === 'number' && isFinite(o),
      'expected ' + t + ' to be a finite number',
      'expected ' + t + ' to not be a finite number'
    );
  });
  property('exist', function () {
    var o = flag(this, 'object');
    var t = display(o);
    this.assert(o !== null && o !== undefined, 'expected ' + t + ' to exist', 'expected ' + t + ' to not exist');
  });
  property('exists', function () {
    var o = flag(this, 'object');
    var t = display(o);
    this.assert(o !== null && o !== undefined, 'expected ' + t + ' to exist', 'expected ' + t + ' to not exist');
  });
  property('empty', function () {
    var o = flag(this, 'object');
    var t = typeOf(o);
    var n;
    if (t === 'string' || t === 'array') n = o.length;
    else if (t === 'map' || t === 'set') n = o.size;
    else if (o !== null && typeof o === 'object') n = Object.keys(o).length;
    else throw new AssertionError('.empty was passed non-string primitive ' + display(o));
    this.assert(n === 0, 'expected ' + display(o) + ' to be empty', 'expected ' + display(o) + ' not to be empty');
  });

  function assertType(type, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    type = String(type).toLowerCase();
    var article = /^[aeiou]/.test(type) ? 'an ' : 'a ';
    this.assert(
      typeOf(o) === type,
      'expected ' + display(o) + ' to be ' + article + type,
      'expected ' + display(o) + ' not to be ' + article + type
    );
  }
  chainable('a', assertType);
  chainable('an', assertType);

  function lengthOfTarget(o) {
    var t = typeOf(o);
    if (t === 'map' || t === 'set') return o.size;
    return o === null || o === undefined ? undefined : o.length;
  }

  function assertInclude(val, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    var deep = flag(this, 'deep');
    var t = typeOf(o);
    var ok = false;
    var i;
    var desc = (deep ? 'deep ' : '') + 'include ';
    if (t === 'string') {
      ok = o.indexOf(val) !== -1;
    } else if (t === 'array') {
      for (i = 0; i < o.length && !ok; i++) ok = deep ? deepEqual(o[i], val) : o[i] === val;
    } else if (t === 'set') {
      ok = deep ? Array.from(o).some(function (x) { return deepEqual(x, val); }) : o.has(val);
    } else if (t === 'map') {
      ok = Array.from(o.values()).some(function (x) { return deep ? deepEqual(x, val) : x === val; });
    } else if (o !== null && typeof o === 'object' && val !== null && typeof val === 'object') {
      var keys = Object.keys(val);
      ok = true;
      for (i = 0; i < keys.length && ok; i++) {
        var k = keys[i];
        ok = k in o && (deep ? deepEqual(o[k], val[k]) : o[k] === val[k]);
      }
    }
    this.assert(
      ok,
      'expected ' + display(o) + ' to ' + desc + display(val),
      'expected ' + display(o) + ' to not ' + desc + display(val)
    );
  }
  function containsFlag() {
    flag(this, 'contains', true);
  }
  ['include', 'includes', 'contain', 'contains'].forEach(function (name) {
    chainable(name, assertInclude, containsFlag);
  });

  function assertLength(n, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    var len = lengthOfTarget(o);
    this.assert(
      len == n,
      'expected ' + display(o) + ' to have a length of ' + display(n) + ' but got ' + display(len),
      'expected ' + display(o) + ' to not have a length of ' + display(len)
    );
  }
  function doLength() {
    flag(this, 'doLength', true);
  }
  chainable('length', assertLength, doLength);
  chainable('lengthOf', assertLength, doLength);

  assertion(['equal', 'equals', 'eq'], function (val, message) {
    withMessage(this, message);
    if (flag(this, 'deep')) return assertEql.call(this, val);
    var o = flag(this, 'object');
    this.assert(
      o === val,
      'expected ' + display(o) + ' to equal ' + display(val),
      'expected ' + display(o) + ' to not equal ' + display(val)
    );
  });

  function assertEql(val, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    this.assert(
      deepEqual(o, val),
      'expected ' + display(o) + ' to deeply equal ' + display(val),
      'expected ' + display(o) + ' to not deeply equal ' + display(val)
    );
  }
  assertion(['eql', 'eqls'], assertEql);

  /** above / least / below / most, on the value or (after `.length`) its length. */
  function comparison(names, test, word, negatedWord) {
    assertion(names, function (n, message) {
      withMessage(this, message);
      var o = flag(this, 'object');
      if (flag(this, 'doLength')) {
        var len = lengthOfTarget(o);
        this.assert(
          test(len, n),
          'expected ' + display(o) + ' to have a length ' + word + ' ' + display(n) + ' but got ' + display(len),
          'expected ' + display(o) + ' to not have a length ' + word + ' ' + display(n)
        );
        return;
      }
      if (typeof o !== 'number' && !(o instanceof Date)) {
        throw new AssertionError(
          (flag(this, 'message') ? flag(this, 'message') + ': ' : '') +
            'expected ' + display(o) + ' to be a number or a date'
        );
      }
      this.assert(
        test(o, n),
        'expected ' + display(o) + ' to be ' + word + ' ' + display(n),
        'expected ' + display(o) + ' to be ' + negatedWord + ' ' + display(n)
      );
    });
  }
  comparison(['above', 'gt', 'greaterThan'], function (a, b) { return a > b; }, 'above', 'at most');
  comparison(['least', 'gte', 'greaterThanOrEqual'], function (a, b) { return a >= b; }, 'at least', 'below');
  comparison(['below', 'lt', 'lessThan'], function (a, b) { return a < b; }, 'below', 'at least');
  comparison(['most', 'lte', 'lessThanOrEqual'], function (a, b) { return a <= b; }, 'at most', 'above');

  assertion(['within'], function (start, finish, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    var v = flag(this, 'doLength') ? lengthOfTarget(o) : o;
    var range = display(start) + '..' + display(finish);
    this.assert(
      v >= start && v <= finish,
      'expected ' + display(o) + ' to be within ' + range,
      'expected ' + display(o) + ' to not be within ' + range
    );
  });

  function pathParts(name) {
    return String(name)
      .replace(/\[(\d+)\]/g, '.$1')
      .split('.')
      .filter(function (p) {
        return p !== '';
      });
  }

  function pathInfo(o, name) {
    var parts = pathParts(name);
    var cur = o;
    for (var i = 0; i < parts.length; i++) {
      if (cur === null || cur === undefined || !(parts[i] in Object(cur))) return { exists: false, value: undefined };
      cur = cur[parts[i]];
    }
    return { exists: true, value: cur };
  }

  assertion(['property'], function (name, val, message) {
    var hasValue = arguments.length > 1;
    withMessage(this, message);
    var o = flag(this, 'object');
    var nested = flag(this, 'nested');
    var isOwn = flag(this, 'own');
    var deep = flag(this, 'deep');
    var info;
    if (nested) info = pathInfo(o, name);
    else if (o === null || o === undefined) info = { exists: false, value: undefined };
    else info = { exists: isOwn ? own(o, name) : name in Object(o), value: o[name] };
    var desc = (deep ? 'deep ' : '') + (isOwn ? 'own ' : '') + (nested ? 'nested ' : '') + 'property ';
    var t = display(o);
    if (!flag(this, 'negate') || !hasValue) {
      this.assert(
        info.exists,
        'expected ' + t + ' to have ' + desc + display(name),
        'expected ' + t + ' to not have ' + desc + display(name)
      );
    }
    if (hasValue) {
      this.assert(
        info.exists && (deep ? deepEqual(val, info.value) : val === info.value),
        'expected ' + t + ' to have ' + desc + display(name) + ' of ' + display(val) + ', but got ' + display(info.value),
        'expected ' + t + ' to not have ' + desc + display(name) + ' of ' + display(val)
      );
    }
    flag(this, 'object', info.value);
  });
  assertion(['ownProperty', 'haveOwnProperty'], function (name, val, message) {
    flag(this, 'own', true);
    return Assertion.prototype.property.apply(this, arguments);
  });

  assertion(['match', 'matches'], function (re, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    this.assert(
      re.exec(o),
      'expected ' + display(o) + ' to match ' + re,
      'expected ' + display(o) + ' not to match ' + re
    );
  });

  assertion(['string'], function (str, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    this.assert(
      typeof o === 'string' && o.indexOf(str) !== -1,
      'expected ' + display(o) + ' to contain ' + display(str),
      'expected ' + display(o) + ' to not contain ' + display(str)
    );
  });

  assertion(['oneOf'], function (list, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    var deep = flag(this, 'deep');
    var contains = flag(this, 'contains');
    var ok = (list || []).some(function (x) {
      if (contains) return typeof o === 'string' ? o.indexOf(x) !== -1 : Array.isArray(o) && o.indexOf(x) !== -1;
      return deep ? deepEqual(x, o) : x === o;
    });
    this.assert(
      ok,
      'expected ' + display(o) + ' to be one of ' + display(list),
      'expected ' + display(o) + ' to not be one of ' + display(list)
    );
  });

  function listText(items, conjunction) {
    items = items.map(function (k) {
      return inspect(k);
    });
    if (items.length < 2) return items.join('');
    var last = items.pop();
    return items.join(', ') + ', ' + conjunction + ' ' + last;
  }

  assertion(['keys', 'key'], function (keys) {
    var o = flag(this, 'object');
    var t = typeOf(o);
    var actual = t === 'map' || t === 'set' ? Array.from(o.keys()) : o === null || o === undefined ? [] : Object.keys(o);
    var expected;
    if (arguments.length === 1 && Array.isArray(keys)) expected = keys.slice();
    else if (arguments.length === 1 && typeOf(keys) === 'object') expected = Object.keys(keys);
    else expected = Array.prototype.slice.call(arguments);
    if (t !== 'map' && t !== 'set') expected = expected.map(String);
    if (!expected.length) throw new Error('keys required');
    var any = flag(this, 'any');
    var contains = flag(this, 'contains');
    var ok;
    if (any) {
      ok = expected.some(function (k) { return actual.indexOf(k) !== -1; });
    } else {
      ok = expected.every(function (k) { return actual.indexOf(k) !== -1; });
      if (!contains) ok = ok && actual.length === expected.length;
    }
    var what = (expected.length > 1 ? 'keys ' : 'key ') + listText(expected, any ? 'or' : 'and');
    what = (any ? 'have any of ' : contains ? 'contain ' : 'have ') + what;
    this.assert(ok, 'expected ' + display(o) + ' to ' + what, 'expected ' + display(o) + ' to not ' + what);
  });

  function isSubsetOf(subset, superset, cmp, contains, ordered) {
    if (!contains) {
      if (subset.length !== superset.length) return false;
      superset = superset.slice();
    }
    return subset.every(function (elem, idx) {
      if (ordered) return cmp(elem, superset[idx]);
      if (!contains) {
        for (var i = 0; i < superset.length; i++) {
          if (cmp(elem, superset[i])) {
            superset.splice(i, 1);
            return true;
          }
        }
        return false;
      }
      return superset.some(function (x) {
        return cmp(elem, x);
      });
    });
  }

  assertion(['members'], function (subset, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    var contains = flag(this, 'contains');
    var ordered = flag(this, 'ordered');
    var cmp = flag(this, 'deep') ? function (a, b) { return deepEqual(a, b); } : function (a, b) { return a === b; };
    var subject;
    var positive;
    var negative;
    if (contains) {
      subject = ordered ? 'an ordered superset' : 'a superset';
      positive = 'to be ' + subject + ' of ';
      negative = 'to not be ' + subject + ' of ';
    } else {
      subject = ordered ? 'ordered members' : 'members';
      positive = 'to have the same ' + subject + ' as ';
      negative = 'to not have the same ' + subject + ' as ';
    }
    var ok = Array.isArray(o) && Array.isArray(subset) && isSubsetOf(subset, o, cmp, contains, ordered);
    this.assert(
      ok,
      'expected ' + display(o) + ' ' + positive + display(subset),
      'expected ' + display(o) + ' ' + negative + display(subset)
    );
  });

  assertion(['instanceOf', 'instanceof'], function (ctor, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    var name = (ctor && ctor.name) || String(ctor);
    this.assert(
      o instanceof ctor,
      'expected ' + display(o) + ' to be an instance of ' + name,
      'expected ' + display(o) + ' to not be an instance of ' + name
    );
  });

  assertion(['closeTo', 'approximately'], function (expected, delta, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    this.assert(
      Math.abs(o - expected) <= delta,
      'expected ' + display(o) + ' to be close to ' + expected + ' +/- ' + delta,
      'expected ' + display(o) + ' not to be close to ' + expected + ' +/- ' + delta
    );
  });

  assertion(['satisfy', 'satisfies'], function (matcher, message) {
    withMessage(this, message);
    var o = flag(this, 'object');
    var name = display(matcher);
    this.assert(matcher(o), 'expected ' + display(o) + ' to satisfy ' + name, 'expected ' + display(o) + ' to not satisfy ' + name);
  });

  assertion(['throw', 'throws', 'Throw'], function (errorLike, matcher, message) {
    withMessage(this, message);
    var fn = flag(this, 'object');
    if (typeof fn !== 'function') throw new AssertionError('expected ' + display(fn) + ' to be a function');
    if (typeof errorLike === 'string' || errorLike instanceof RegExp) {
      matcher = errorLike;
      errorLike = undefined;
    }
    var thrown = false;
    var caught;
    try {
      fn();
    } catch (e) {
      thrown = true;
      caught = e;
    }
    var t = display(fn);
    if (!errorLike && matcher === undefined) {
      this.assert(
        thrown,
        'expected ' + t + ' to throw an error',
        'expected ' + t + ' to not throw an error but ' + display(caught) + ' was thrown'
      );
    } else {
      var text = caught && caught.message !== undefined ? String(caught.message) : String(caught);
      var typeOk = !errorLike || caught instanceof errorLike || caught === errorLike;
      var textOk =
        matcher === undefined || (matcher instanceof RegExp ? matcher.test(text) : text.indexOf(matcher) !== -1);
      var what = errorLike ? (errorLike.name || display(errorLike)) : 'an error';
      if (matcher !== undefined) what += ' including ' + display(matcher);
      this.assert(
        thrown && typeOk && textOk,
        'expected ' + t + ' to throw ' + what + (thrown ? ' but ' + display(caught) + ' was thrown' : ''),
        'expected ' + t + ' to not throw ' + what
      );
    }
    flag(this, 'object', caught);
  });

  // ---- pm.response.to (chai-postman subset) --------------------------------------

  function ResponseAssertion(response) {
    Assertion.call(this, response);
  }
  ResponseAssertion.prototype = Object.create(Assertion.prototype);

  var responses = new WeakSet();

  function responseBodyJson(res) {
    try {
      return { ok: true, value: jsonParse(res.text()) };
    } catch (e) {
      return { ok: false, error: e };
    }
  }

  var statusCodes = {
    ok: 200,
    accepted: 202,
    withoutContent: 204,
    badRequest: 400,
    unauthorized: 401,
    unauthorised: 401,
    forbidden: 403,
    notFound: 404,
    notAcceptable: 406,
    rateLimited: 429,
  };
  Object.keys(statusCodes).forEach(function (name) {
    define(ResponseAssertion.prototype, name, function () {
      var code = flag(this, 'object').code;
      var want = statusCodes[name];
      this.assert(
        code === want,
        'expected response code to be ' + want + ' but found ' + code,
        'expected response code to not be ' + want
      );
      return this;
    });
  });
  var statusClasses = { info: 1, success: 2, redirection: 3, clientError: 4, serverError: 5 };
  Object.keys(statusClasses).forEach(function (name) {
    define(ResponseAssertion.prototype, name, function () {
      var code = flag(this, 'object').code;
      var want = statusClasses[name];
      this.assert(
        Math.floor(code / 100) === want,
        'expected response code to be ' + want + 'XX but found ' + code,
        'expected response code to not be ' + want + 'XX'
      );
      return this;
    });
  });
  define(ResponseAssertion.prototype, 'error', function () {
    var code = flag(this, 'object').code;
    var cls = Math.floor(code / 100);
    this.assert(
      cls === 4 || cls === 5,
      'expected response code to be 4XX or 5XX but found ' + code,
      'expected response code to not be 4XX or 5XX'
    );
    return this;
  });
  define(ResponseAssertion.prototype, 'withBody', function () {
    this.assert(
      flag(this, 'object').text().length > 0,
      'expected response to have content in body',
      'expected response to not have content in body'
    );
    return this;
  });
  define(ResponseAssertion.prototype, 'json', function () {
    var parsed = responseBodyJson(flag(this, 'object'));
    this.assert(
      parsed.ok,
      'expected response body to be a valid json but got error ' + (parsed.ok ? '' : errorText(parsed.error)),
      'expected response body not to be a valid json'
    );
    return this;
  });
  method(ResponseAssertion.prototype, 'status', function (want) {
    var res = flag(this, 'object');
    if (typeof want === 'number') {
      this.assert(
        res.code === want,
        'expected response to have status code ' + want + ' but got ' + res.code,
        'expected response to not have status code ' + want
      );
    } else {
      this.assert(
        res.status === String(want),
        'expected response to have status reason ' + inspect(String(want)) + ' but got ' + inspect(res.status),
        'expected response to not have status reason ' + inspect(String(want))
      );
    }
    return this;
  });
  method(ResponseAssertion.prototype, 'header', function (name, value) {
    var res = flag(this, 'object');
    var has = res.headers.has(name);
    if (value === undefined) {
      this.assert(
        has,
        "expected response to have header with key '" + name + "'",
        "expected response to not have header with key '" + name + "'"
      );
    } else {
      var actual = res.headers.get(name);
      this.assert(
        has && actual === String(value),
        "expected '" + name + "' response header to be '" + value + "' but got '" + actual + "'",
        "expected '" + name + "' response header to not be '" + value + "'"
      );
    }
    return this;
  });
  method(ResponseAssertion.prototype, 'body', function (want) {
    var res = flag(this, 'object');
    var text = res.text();
    if (arguments.length === 0) {
      this.assert(text.length > 0, 'expected response to have content in body', 'expected response to not have content in body');
    } else if (want instanceof RegExp) {
      this.assert(want.test(text), 'expected response body to match ' + want, 'expected response body to not match ' + want);
    } else if (typeof want === 'string') {
      this.assert(
        text === want,
        'expected response body to equal ' + display(want) + ' but got ' + display(text),
        'expected response body to not equal ' + display(want)
      );
    } else {
      var parsed = responseBodyJson(res);
      this.assert(
        parsed.ok && deepEqual(parsed.value, want),
        'expected response body json to equal ' + display(want) + ' but got ' + display(parsed.value),
        'expected response body json to not equal ' + display(want)
      );
    }
    return this;
  });
  method(ResponseAssertion.prototype, 'jsonBody', function (path, value) {
    var parsed = responseBodyJson(flag(this, 'object'));
    if (!parsed.ok || arguments.length === 0) {
      this.assert(
        parsed.ok,
        'expected response body to be a valid json but got error ' + (parsed.ok ? '' : errorText(parsed.error)),
        'expected response body not to be a valid json'
      );
      return this;
    }
    if (typeof path !== 'string') {
      this.assert(
        deepEqual(parsed.value, path),
        'expected response body json to equal ' + display(path) + ' but got ' + display(parsed.value),
        'expected response body json to not equal ' + display(path)
      );
      return this;
    }
    var info = pathInfo(parsed.value, path);
    if (arguments.length === 1) {
      this.assert(
        info.exists,
        'expected ' + display(parsed.value) + ' to have property ' + inspect(path),
        'expected ' + display(parsed.value) + ' to not have property ' + inspect(path)
      );
    } else {
      this.assert(
        info.exists && deepEqual(info.value, value),
        'expected response body json at ' + inspect(path) + ' to equal ' + display(value) + ' but got ' + display(info.value),
        'expected response body json at ' + inspect(path) + ' to not equal ' + display(value)
      );
    }
    return this;
  });
  /** The run's libraries (`setup` sets them): `jsonSchema` validates with the bundled Ajv. */
  var runLibraries = null;

  method(ResponseAssertion.prototype, 'jsonSchema', function (schema, options) {
    var parsed = responseBodyJson(flag(this, 'object'));
    if (!parsed.ok) {
      this.assert(false, 'expected response body to be a valid json but got error ' + errorText(parsed.error), '');
      return this;
    }
    var Ajv = runLibraries.require('ajv');
    var ajv = new Ajv(Object.assign({ allErrors: true }, options || {}));
    var valid = ajv.validate(schema, parsed.value);
    this.assert(
      valid,
      'expected response body to match the JSON Schema: ' + ajv.errorsText(ajv.errors, { dataVar: 'body' }),
      'expected response body to not match the JSON Schema'
    );
    return this;
  });

  function expect(value, message) {
    if (responses.has(value)) {
      var a = new ResponseAssertion(value);
      flag(a, 'message', message);
      return a;
    }
    return new Assertion(value, message);
  }

  // ---- request / response objects ------------------------------------------------

  function parseHeader(h, value) {
    if (typeof h === 'string') {
      if (value !== undefined) return { key: h, value: stored(value) };
      var i = h.indexOf(':');
      return i < 0 ? { key: h.trim(), value: '' } : { key: h.slice(0, i).trim(), value: h.slice(i + 1).trim() };
    }
    if (h && typeof h === 'object') {
      return { key: String(h.key !== undefined ? h.key : h.name), value: stored(h.value) };
    }
    throw new TypeError('A header is {key, value} or "Name: value"');
  }

  /** A Postman PropertyList of `{key, value}`: case-insensitive for headers. */
  function propertyList(list, parse, caseless, changed) {
    function same(a, b) {
      return caseless ? String(a).toLowerCase() === String(b).toLowerCase() : String(a) === String(b);
    }
    function find(key) {
      for (var i = 0; i < list.length; i++) if (same(list[i].key, key)) return list[i];
      return undefined;
    }
    function all() {
      return list.map(function (p) {
        return { key: p.key, value: p.value };
      });
    }
    var api = {
      get: function (key) {
        var p = find(key);
        return p ? p.value : undefined;
      },
      one: function (key) {
        var p = find(key);
        return p ? { key: p.key, value: p.value } : undefined;
      },
      has: function (key, value) {
        return list.some(function (p) {
          return same(p.key, key) && (value === undefined || p.value === String(value));
        });
      },
      all: all,
      toObject: function () {
        var o = {};
        list.forEach(function (p) {
          o[p.key] = p.value;
        });
        return o;
      },
      each: function (fn, ctx) {
        all().forEach(fn, ctx);
      },
      map: function (fn, ctx) {
        return all().map(fn, ctx);
      },
      filter: function (fn, ctx) {
        return all().filter(fn, ctx);
      },
      find: function (fn, ctx) {
        return all().find(fn, ctx);
      },
      count: function () {
        return list.length;
      },
      add: function (item, value) {
        list.push(parse(item, value));
        changed();
      },
      append: function (item, value) {
        api.add(item, value);
      },
      upsert: function (item, value) {
        var p = parse(item, value);
        var existing = find(p.key);
        if (existing) existing.value = p.value;
        else list.push(p);
        changed();
      },
      remove: function (key) {
        for (var i = list.length - 1; i >= 0; i--) {
          var p = list[i];
          if (typeof key === 'function' ? key({ key: p.key, value: p.value }) : same(p.key, key)) list.splice(i, 1);
        }
        changed();
      },
      clear: function () {
        list.length = 0;
        changed();
      },
      toJSON: all,
    };
    return api;
  }

  function headerList(list) {
    var api = propertyList(list, parseHeader, true, function () {});
    api.toString = function () {
      return list
        .map(function (h) {
          return h.key + ': ' + h.value;
        })
        .join('\n');
    };
    return api;
  }

  var URL_PARTS = /^([^?#]*)(?:\?([^#]*))?(?:#([\s\S]*))?$/;
  var URL_BASE = /^(?:([A-Za-z][A-Za-z0-9+.-]*):\/\/)?(?:([^@\/]*)@)?(\[[^\]]*\]|[^\/:]*)(?::([^\/]*))?([\s\S]*)$/;

  /** `pm.request.url`: reads and edits the request's URL string. */
  function urlObject(req) {
    function parts() {
      var m = URL_PARTS.exec(req.url) || [req.url, req.url];
      var b = URL_BASE.exec(m[1]) || [];
      return {
        protocol: b[1],
        auth: b[2],
        host: b[3] || '',
        port: b[4],
        path: b[5] || '',
        query: m[2],
        hash: m[3],
      };
    }
    function parseParam(item, value) {
      if (typeof item === 'string') {
        if (value !== undefined) return { key: item, value: value === null ? null : stored(value) };
        var i = item.indexOf('=');
        return i < 0 ? { key: item, value: null } : { key: item.slice(0, i), value: item.slice(i + 1) };
      }
      if (item && typeof item === 'object') {
        return { key: String(item.key), value: item.value === null || item.value === undefined ? null : stored(item.value) };
      }
      throw new TypeError('A query parameter is {key, value} or "key=value"');
    }
    var params = [];
    function readParams() {
      var q = parts().query;
      params.length = 0;
      if (q) q.split('&').forEach(function (p) { params.push(parseParam(p)); });
    }
    function writeParams() {
      var m = URL_PARTS.exec(req.url) || [req.url, req.url];
      var q = params
        .map(function (p) {
          return p.value === null ? p.key : p.key + '=' + p.value;
        })
        .join('&');
      req.url = m[1] + (params.length ? '?' + q : '') + (m[3] !== undefined ? '#' + m[3] : '');
    }
    var query = propertyList(params, parseParam, false, writeParams);
    // Every read starts from the current URL string.
    Object.keys(query).forEach(function (name) {
      var fn = query[name];
      query[name] = function () {
        readParams();
        return fn.apply(query, arguments);
      };
    });
    query.toString = function () {
      return parts().query || '';
    };
    var url = {
      toString: function () {
        return req.url;
      },
      toJSON: function () {
        return req.url;
      },
      update: function (s) {
        req.url = stored(s);
      },
      getHost: function () {
        return parts().host;
      },
      getPath: function () {
        return parts().path || '/';
      },
      getQueryString: function () {
        return parts().query || '';
      },
      getPathWithQuery: function () {
        var p = parts();
        return (p.path || '/') + (p.query ? '?' + p.query : '');
      },
      getRemote: function () {
        var p = parts();
        return p.host + (p.port ? ':' + p.port : '');
      },
      query: query,
    };
    define(url, 'protocol', function () {
      return parts().protocol;
    });
    define(url, 'host', function () {
      return parts().host.split('.');
    });
    define(url, 'port', function () {
      return parts().port;
    });
    define(url, 'path', function () {
      return parts()
        .path.split('/')
        .filter(function (s) {
          return s !== '';
        });
    });
    define(url, 'hash', function () {
      return parts().hash;
    });
    return url;
  }

  function requestObject(req) {
    var url = urlObject(req);
    var headers = headerList(req.headers);
    var body = {
      update: function (v) {
        if (v && typeof v === 'object') {
          if ('raw' in v) req.body = stored(v.raw);
        } else {
          req.body = stored(v);
        }
      },
      isEmpty: function () {
        return !req.body;
      },
      toString: function () {
        return req.body;
      },
      toJSON: function () {
        return { mode: 'raw', raw: req.body };
      },
    };
    define(body, 'mode', function () {
      return 'raw';
    });
    Object.defineProperty(body, 'raw', {
      get: function () {
        return req.body;
      },
      set: function (v) {
        req.body = stored(v);
      },
    });
    var request = {
      headers: headers,
      body: body,
      addHeader: function (h) {
        headers.add(h);
      },
      removeHeader: function (name) {
        headers.remove(name);
      },
      upsertHeader: function (h) {
        headers.upsert(h);
      },
      getHeaders: function () {
        return headers.toObject();
      },
      toJSON: function () {
        return { url: req.url, method: req.method, header: headers.all(), body: body.toJSON() };
      },
    };
    Object.defineProperty(request, 'url', {
      get: function () {
        return url;
      },
      set: function (v) {
        req.url = stored(typeof v === 'object' && v !== null ? String(v) : v);
      },
    });
    Object.defineProperty(request, 'method', {
      get: function () {
        return req.method;
      },
      set: function (v) {
        req.method = String(v).toUpperCase();
      },
    });
    return request;
  }

  function responseObject(res, body) {
    var response = {
      code: res.code,
      status: res.status,
      responseTime: res.responseTime,
      responseSize: res.responseSize,
      headers: headerList(res.headers.slice()),
      text: function () {
        return body;
      },
      json: function () {
        try {
          return jsonParse(body);
        } catch (e) {
          // Out of memory (thrown as `null`) or too deeply nested: not a syntax problem.
          if (!(e instanceof SyntaxError)) throw e;
          throw new SyntaxError('pm.response.json(): the response body is not valid JSON (' + e.message + ')');
        }
      },
      reason: function () {
        return res.status;
      },
      toJSON: function () {
        return { code: res.code, status: res.status, header: response.headers.all(), body: body };
      },
    };
    define(response, 'to', function () {
      return new ResponseAssertion(response);
    });
    var cookies = cookieList(res.cookies || []);
    define(response, 'cookies', function () {
      return cookies;
    });
    // Event streams (collection runs): the events read, as {event, data, id}.
    if (res.events) {
      var events = res.events.map(function (e) {
        return { event: e.event, data: e.data, id: e.id === undefined ? null : e.id };
      });
      define(response, 'events', function () {
        return events;
      });
    }
    responses.add(response);
    return response;
  }

  // ---- cookies ---------------------------------------------------------------------

  /** `pm.cookies` and `pm.response.cookies`: a read-only list of `{name, value, domain, …}`. */
  function cookieList(cookies) {
    var list = cookies.map(function (c) {
      return {
        name: c.name,
        key: c.name,
        value: c.value,
        domain: c.domain || '',
        path: c.path || '/',
        expires: c.expires || null,
        secure: !!c.secure,
        httpOnly: !!c.httpOnly,
      };
    });
    function one(name) {
      for (var i = 0; i < list.length; i++) if (list[i].name === String(name)) return list[i];
      return undefined;
    }
    return {
      get: function (name) {
        var c = one(name);
        return c ? c.value : undefined;
      },
      has: function (name, value) {
        var c = one(name);
        return !!c && (value === undefined || c.value === String(value));
      },
      one: one,
      all: function () {
        return list.slice();
      },
      count: function () {
        return list.length;
      },
      each: function (fn) {
        list.forEach(fn);
      },
      filter: function (fn) {
        return list.filter(fn);
      },
      map: function (fn) {
        return list.map(fn);
      },
      toObject: function () {
        var o = {};
        list.forEach(function (c) {
          o[c.name] = c.value;
        });
        return o;
      },
      toString: function () {
        return list
          .map(function (c) {
            return c.name + '=' + c.value;
          })
          .join('; ');
      },
    };
  }

  /**
   * Hands a host reply (`{ok, …}` JSON) to a Node-style callback, on a later tick like
   * Postman's, or as a promise when there is no callback.
   */
  function settle(replyJson, pick, callback) {
    var reply = jsonParse(replyJson);
    var error = reply.ok ? null : new Error(reply.error);
    var value = reply.ok ? pick(reply) : undefined;
    if (typeof callback === 'function') {
      Promise.resolve().then(function () {
        callback(error, value);
      });
      return undefined;
    }
    return error ? Promise.reject(error) : Promise.resolve(value);
  }

  /** `pm.cookies.jar()`: the workspace's cookie jar (only for the request's own site). */
  function cookieJar(host) {
    var none = function () {
      return undefined;
    };
    return {
      get: function (url, name, callback) {
        return settle(
          host.cookies(String(url)),
          function (r) {
            var c = r.cookies.filter(function (x) {
              return x.name === String(name);
            })[0];
            return c ? c.value : undefined;
          },
          callback
        );
      },
      getAll: function (url, options, callback) {
        if (typeof options === 'function') callback = options;
        return settle(
          host.cookies(String(url)),
          function (r) {
            return cookieList(r.cookies).all();
          },
          callback
        );
      },
      set: function (url, name, value, callback) {
        if (name && typeof name === 'object') {
          callback = value;
          value = name.value;
          name = name.name || name.key;
        } else if (typeof value === 'function') {
          callback = value;
          value = '';
        }
        return settle(host.cookie('set', String(url), String(name), value == null ? '' : String(value)), none, callback);
      },
      unset: function (url, name, callback) {
        return settle(host.cookie('unset', String(url), String(name)), none, callback);
      },
      clear: function (url, callback) {
        return settle(host.cookie('clear', String(url)), none, callback);
      },
    };
  }

  // ---- pm.sendRequest ------------------------------------------------------------

  /** Header entries of any Postman shape: `[{key, value, disabled}]`, `{name: value}` or "k: v" lines. */
  function requestHeaders(h) {
    var out = [];
    if (!h) return out;
    if (typeof h === 'string') {
      h.split(/\r?\n/).forEach(function (line) {
        var i = line.indexOf(':');
        if (i > 0) out.push({ key: line.slice(0, i).trim(), value: line.slice(i + 1).trim() });
      });
    } else if (Array.isArray(h)) {
      h.forEach(function (x) {
        if (x && !x.disabled && x.key !== undefined) out.push({ key: String(x.key), value: x.value == null ? '' : String(x.value) });
      });
    } else if (typeof h === 'object') {
      if (typeof h.toObject === 'function') h = h.toObject();
      for (var k in h) if (own(h, k)) out.push({ key: k, value: h[k] == null ? '' : String(h[k]) });
    }
    return out;
  }

  function hasHeader(headers, name) {
    for (var i = 0; i < headers.length; i++) if (headers[i].key.toLowerCase() === name) return true;
    return false;
  }

  function formEncode(s) {
    return encodeURIComponent(s).replace(/%20/g, '+');
  }

  /** What `pm.sendRequest` sends: a URL string or a Postman request object. */
  function outgoing(req) {
    if (typeof req === 'string') return { method: 'GET', url: req, headers: [], body: '' };
    if (!req || typeof req !== 'object') throw new TypeError('pm.sendRequest: give a URL or a request object');
    var url = req.url;
    if (url && typeof url === 'object') url = url.raw !== undefined ? url.raw : String(url);
    var headers = requestHeaders(req.header || req.headers);
    var body = '';
    var b = req.body;
    var type = function (value) {
      if (!hasHeader(headers, 'content-type')) headers.push({ key: 'Content-Type', value: value });
    };
    if (typeof b === 'string') {
      body = b;
    } else if (b && typeof b === 'object') {
      var fields = function (list) {
        return (list || []).filter(function (f) {
          return f && !f.disabled && f.key !== undefined;
        });
      };
      switch (b.mode) {
        case 'raw':
          body = typeof b.raw === 'string' ? b.raw : b.raw === undefined ? '' : jsonStringify(b.raw);
          if (b.options && b.options.raw && b.options.raw.language === 'json') type('application/json');
          break;
        case 'urlencoded':
          body = fields(b.urlencoded)
            .map(function (f) {
              return formEncode(String(f.key)) + '=' + formEncode(f.value == null ? '' : String(f.value));
            })
            .join('&');
          type('application/x-www-form-urlencoded');
          break;
        case 'formdata':
          var boundary = '----zorvik' + Math.random().toString(16).slice(2);
          fields(b.formdata).forEach(function (f) {
            if (f.type === 'file' || f.src !== undefined) throw new Error('pm.sendRequest: form-data files are not supported; send text fields');
            body += '--' + boundary + '\r\nContent-Disposition: form-data; name="' + String(f.key).replace(/"/g, '%22') + '"\r\n\r\n';
            body += (f.value == null ? '' : String(f.value)) + '\r\n';
          });
          body += '--' + boundary + '--\r\n';
          type('multipart/form-data; boundary=' + boundary);
          break;
        case 'graphql':
          var g = b.graphql || {};
          var variables = g.variables;
          if (typeof variables === 'string') variables = variables.trim() ? jsonParse(variables) : undefined;
          body = jsonStringify({ query: g.query || '', variables: variables });
          type('application/json');
          break;
        case 'file':
          throw new Error('pm.sendRequest: file bodies are not supported');
      }
    }
    return { method: String(req.method || 'GET').toUpperCase(), url: stored(url), headers: headers, body: stored(body) };
  }

  function sendRequest(host, req, callback) {
    var r = outgoing(req);
    var reply = host.send(jsonStringify(r));
    var parsed = jsonParse(reply);
    var line = 'pm.sendRequest ' + r.method + ' ' + r.url + ' → ' + (parsed.ok ? parsed.response.code + ' ' + parsed.response.status : parsed.error);
    if (logs.length < MAX_CONSOLE) logs.push({ level: 'info', message: clip(line) });
    return settle(
      reply,
      function (x) {
        return responseObject(x.response, x.response.body);
      },
      callback
    );
  }

  // ---- base64 (atob / btoa, if the engine has none) --------------------------------

  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

  function btoa(input) {
    var s = String(input);
    var out = '';
    for (var i = 0; i < s.length; i += 3) {
      var a = s.charCodeAt(i);
      var b = i + 1 < s.length ? s.charCodeAt(i + 1) : 0;
      var c = i + 2 < s.length ? s.charCodeAt(i + 2) : 0;
      if (a > 255 || b > 255 || c > 255) throw new Error('btoa: the string has characters outside of Latin1');
      var n = (a << 16) | (b << 8) | c;
      out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63];
      out += i + 1 < s.length ? B64[(n >> 6) & 63] : '=';
      out += i + 2 < s.length ? B64[n & 63] : '=';
    }
    return out;
  }

  function atob(input) {
    var s = String(input).replace(/[\t\n\f\r ]/g, '');
    if (s.length % 4 === 0) s = s.replace(/==?$/, '');
    if (s.length % 4 === 1 || /[^A-Za-z0-9+\/]/.test(s)) throw new Error('atob: the string is not valid base64');
    var out = '';
    var bits = 0;
    var n = 0;
    for (var i = 0; i < s.length; i++) {
      n = (n << 6) | B64.indexOf(s[i]);
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        out += String.fromCharCode((n >> bits) & 255);
        n &= (1 << bits) - 1;
      }
    }
    return out;
  }

  // ---- require: the built-in libraries -------------------------------------------

  // Postman's `xml2Json` options (explicitArray off: single children aren't arrays).
  var XML2JSON = { explicitArray: false, async: false, trim: true, mergeAttrs: false };

  /**
   * `require` for the libraries Zorvik ships (crates/script/src/libs.rs), and the
   * globals Postman's sandbox has for them. `host.library(name)` returns a library
   * as a CommonJS factory (undefined for a name Zorvik doesn't have), `host.random(n)`
   * secure random bytes, `host.names` every name. A library loads on first use and
   * is kept for the rest of the run.
   */
  function libraries(host) {
    var modules = new Map();

    function require(id) {
      if (typeof id !== 'string') throw new TypeError('require: the module name must be a string');
      var name = id.indexOf('node:') === 0 ? id.slice(5) : id;
      var module = modules.get(name);
      if (module) return module.exports;
      var factory = host.library(name);
      if (typeof factory !== 'function') {
        throw new Error(
          "Cannot find module '" + id + "'. Scripts can require only these built-in libraries: " + host.names.join(', ')
        );
      }
      module = { id: name, exports: {}, loaded: false };
      // Kept before it runs, so libraries that require each other get the exports so far (as in Node).
      modules.set(name, module);
      try {
        factory.call(module.exports, module.exports, require, module, name + '.js', '');
      } catch (e) {
        modules.delete(name);
        // Out of memory (QuickJS throws null or a bare InternalError then): as it is, for the engine to report.
        if (!(e instanceof Error) || e.name === 'InternalError') throw e;
        throw new Error('The built-in library "' + name + '" failed to load: ' + errorText(e));
      }
      module.loaded = true;
      return module.exports;
    }

    /** Postman's `pm.require('npm:name@version')`; the version is ignored (Zorvik has one of each). */
    function pmRequire(spec) {
      var s = String(spec);
      var npm = /^npm:((?:@[^\/@]+\/)?[^\/@]+)(?:@[^\/]*)?(\/.*)?$/.exec(s);
      if (npm) return require(npm[1] + (npm[2] || ''));
      if (/^@[^\/]+\/[^\/]+$/.test(s)) {
        throw new Error(
          "pm.require: '" + s + "' is from a Postman package library, which Zorvik can't reach. Copy its code into the script."
        );
      }
      return require(s);
    }

    function getRandomValues(array) {
      if (!ArrayBuffer.isView(array) || array instanceof DataView || /^float/.test(typeOf(array))) {
        throw new TypeError('crypto.getRandomValues: the argument must be an integer typed array');
      }
      new Uint8Array(array.buffer, array.byteOffset, array.byteLength).set(host.random(array.byteLength));
      return array;
    }

    function randomUUID() {
      var b = getRandomValues(new Uint8Array(16));
      b[6] = (b[6] & 0x0f) | 0x40;
      b[8] = (b[8] & 0x3f) | 0x80;
      var hex = '';
      for (var i = 0; i < 16; i++) {
        hex += (b[i] + 0x100).toString(16).slice(1);
        if (i === 3 || i === 5 || i === 7 || i === 9) hex += '-';
      }
      return hex;
    }

    /** A global that loads its library on first use; assigning to it replaces it. */
    function lazy(name, library) {
      function replace(value) {
        Object.defineProperty(G, name, { value: value, writable: true, configurable: true });
      }
      Object.defineProperty(G, name, {
        get: function () {
          // Read by the library itself while it loads (lodash keeps the old `_` for noConflict).
          var loading = modules.get(library);
          if (loading && !loading.loaded) return undefined;
          var value = require(library);
          replace(value);
          return value;
        },
        set: replace,
        configurable: true,
      });
    }

    function install() {
      // Web Crypto's random part: uuid and crypto-js need it; scripts can use it too.
      G.crypto = { getRandomValues: getRandomValues, randomUUID: randomUUID };
      lazy('CryptoJS', 'crypto-js');
      lazy('_', 'lodash');
      lazy('tv4', 'tv4');
      lazy('cheerio', 'cheerio');
      G.xml2Json = function xml2Json(xml) {
        var json;
        var failed = null;
        require('xml2js').parseString(String(xml), XML2JSON, function (e, result) {
          failed = e;
          json = result;
        });
        if (failed) throw new Error('xml2Json: the text is not valid XML (' + String(failed.message || failed).split('\n')[0] + ')');
        return json;
      };
    }

    return { require: require, pmRequire: pmRequire, install: install };
  }

  // ---- setup ---------------------------------------------------------------------

  return function setup(inputJson, responseBody, host) {
    var input = jsonParse(inputJson);
    var vars = input.variables || {};
    var overrides = copy(vars.overrides);
    var local = copy(vars.local);
    var data = copy(vars.data);
    var environment = copy(vars.environment);
    var collection = copy(vars.collection);
    var globals = copy(vars.globals);
    var chain = [overrides, local, data, environment, collection, globals];
    var changes = new Map();
    var tests = [];
    var skip = false;
    var visualization = null;
    hostDynamic = host && typeof host.dynamic === 'function' ? host.dynamic : null;

    // Timers: run after the script, in time order, waiting for the ones in the future.
    var timers = [];
    var timerSeq = 0;
    function addTimer(fn, ms, args, repeat) {
      if (typeof fn !== 'function') throw new TypeError('setTimeout and setInterval need a function');
      var delay = Math.max(0, Number(ms) || 0);
      timerSeq++;
      timers.push({ id: timerSeq, at: Date.now() + delay, fn: fn, args: args, every: repeat ? Math.max(1, delay) : 0 });
      return timerSeq;
    }
    function clearTimer(id) {
      for (var i = 0; i < timers.length; i++) {
        if (timers[i].id === id) {
          timers.splice(i, 1);
          return;
        }
      }
    }
    /** Runs the next timer (waiting for it); false when there is none. */
    function tick() {
      if (!timers.length) return false;
      var next = timers[0];
      for (var i = 1; i < timers.length; i++) if (timers[i].at < next.at) next = timers[i];
      var wait = next.at - Date.now();
      if (wait > 0) host.sleep(wait);
      if (next.every) next.at = Math.max(next.at, Date.now()) + next.every;
      else clearTimer(next.id);
      next.fn.apply(undefined, next.args);
      return true;
    }
    // `pm.execution.setNextRequest`: `{name}` (null stops the iteration); the runner acts on it.
    var nextRequest = null;

    function setNextRequest(name) {
      nextRequest = { name: name === null || name === undefined ? null : String(name) };
    }

    function record(scope, key, value) {
      var id = scope + '\u0000' + key;
      changes.delete(id);
      changes.set(id, { scope: scope, key: key, value: value });
    }

    function lookup(name) {
      for (var i = 0; i < chain.length; i++) if (own(chain[i], name)) return chain[i][name];
      return undefined;
    }

    /** `pm.environment` & co.: `get / set / has / unset / clear / replaceIn / toObject`. */
    function scope(name, values, readOnly) {
      var api = {
        get: function (key) {
          return own(values, String(key)) ? values[String(key)] : undefined;
        },
        has: function (key) {
          return own(values, String(key));
        },
        toObject: function () {
          return copy(values);
        },
        toJSON: function () {
          return copy(values);
        },
        replaceIn: function (template) {
          return render(template, api.get);
        },
      };
      if (!readOnly) {
        api.set = function (key, value) {
          if (key === undefined || key === null) return;
          values[String(key)] = value;
          record(name, String(key), stored(value));
        };
        api.unset = function (key) {
          delete values[String(key)];
          record(name, String(key), null);
        };
        api.clear = function () {
          Object.keys(values).forEach(api.unset);
        };
      }
      return api;
    }

    var variables = scope('local', local);
    variables.get = function (key) {
      return lookup(String(key));
    };
    variables.has = function (key) {
      return lookup(String(key)) !== undefined;
    };
    variables.toObject = function () {
      var o = {};
      for (var i = chain.length - 1; i >= 0; i--) Object.assign(o, chain[i]);
      return o;
    };
    variables.replaceIn = function (template) {
      return render(template, lookup);
    };

    var envScope = scope('environment', environment);
    envScope.name = input.environmentName === null ? undefined : input.environmentName;

    var req = input.request;
    var info = input.info || {};

    function test(name, fn) {
      var entry = { name: String(name), passed: true, skipped: false, error: null, pending: null };
      if (tests.length < MAX_TESTS) tests.push(entry);
      if (typeof fn !== 'function') {
        entry.passed = false;
        entry.skipped = true;
        return;
      }
      function fail(e) {
        entry.passed = false;
        if (entry.error === null) entry.error = clip(errorText(e));
      }
      try {
        if (fn.length > 0) {
          entry.pending = 'done() was never called';
          fn.call(G, function done(err) {
            if (!entry.pending) return;
            entry.pending = null;
            if (err) fail(err);
          });
        } else {
          var r = fn.call(G);
          if (r && typeof r.then === 'function') {
            entry.pending = 'its promise never settled';
            r.then(
              function () {
                entry.pending = null;
              },
              function (e) {
                entry.pending = null;
                fail(e);
              }
            );
          }
        }
      } catch (e) {
        entry.pending = null;
        fail(e);
      }
    }
    test.skip = function (name) {
      if (tests.length < MAX_TESTS) tests.push({ name: String(name), passed: false, skipped: true, error: null });
    };

    var libs = libraries(host);
    runLibraries = libs;
    var pm = {
      info: {
        eventName: input.event,
        iteration: info.iteration || 0,
        iterationCount: info.iterationCount || 1,
        requestName: info.requestName || '',
        requestId: info.requestId || '',
      },
      variables: variables,
      environment: envScope,
      collectionVariables: scope('collection', collection),
      globals: scope('globals', globals),
      iterationData: scope('data', data, true),
      request: requestObject(req),
      response: input.response ? responseObject(input.response, typeof responseBody === 'string' ? responseBody : '') : undefined,
      test: test,
      expect: expect,
      sendRequest: function (req, callback) {
        return sendRequest(host, req, callback);
      },
      require: libs.pmRequire,
      execution: {
        setNextRequest: setNextRequest,
        // Only a pre-request script can skip: the request isn't sent (a run goes on).
        skipRequest: function () {
          if (input.event === 'prerequest') skip = true;
        },
      },
      visualizer: {
        /** Renders a Handlebars `template` with `data`: the response's Visualize tab shows it. */
        set: function (template, data) {
          var html = libs.require('handlebars').compile(String(template))(data === undefined ? {} : data);
          if (html.length > MAX_VISUALIZATION) throw new Error('pm.visualizer: the result is larger than 5 MB');
          visualization = html;
        },
        clear: function () {
          visualization = null;
        },
      },
    };
    var cookies = cookieList(input.cookies || []);
    cookies.jar = function () {
      return cookieJar(host);
    };
    define(pm, 'cookies', function () {
      return cookies;
    });
    define(pm, 'vault', unsupported('pm.vault'));

    G.pm = pm;
    G.console = consoleApi;
    G.require = libs.require;
    G.setTimeout = function (fn, ms) {
      return addTimer(fn, ms, Array.prototype.slice.call(arguments, 2), false);
    };
    G.setInterval = function (fn, ms) {
      return addTimer(fn, ms, Array.prototype.slice.call(arguments, 2), true);
    };
    G.setImmediate = function (fn) {
      return addTimer(fn, 0, Array.prototype.slice.call(arguments, 1), false);
    };
    G.clearTimeout = G.clearInterval = G.clearImmediate = clearTimer;
    // Postman's sandbox libraries (`CryptoJS`, `_`, `tv4`, `cheerio`, `xml2Json`) and `crypto`.
    libs.install();
    if (typeof G.atob !== 'function') G.atob = atob;
    if (typeof G.btoa !== 'function') G.btoa = btoa;

    // Legacy Postman API (`postman.*`, `tests[…]`, `responseBody`, …), still common in old collections.
    G.tests = {};
    G.postman = {
      setEnvironmentVariable: envScope.set,
      getEnvironmentVariable: envScope.get,
      clearEnvironmentVariable: envScope.unset,
      clearEnvironmentVariables: envScope.clear,
      setGlobalVariable: pm.globals.set,
      getGlobalVariable: pm.globals.get,
      clearGlobalVariable: pm.globals.unset,
      clearGlobalVariables: pm.globals.clear,
      getResponseHeader: function (name) {
        return pm.response ? pm.response.headers.get(name) : undefined;
      },
      setNextRequest: setNextRequest,
    };
    G.environment = copy(environment);
    G.globals = copy(globals);
    G.data = copy(data);
    G.iteration = pm.info.iteration;
    if (pm.response) {
      G.responseBody = pm.response.text();
      G.responseCode = { code: pm.response.code, name: pm.response.status, detail: pm.response.status };
      G.responseHeaders = pm.response.headers.toObject();
      G.responseTime = pm.response.responseTime;
    }

    function finish() {
      var i;
      for (i = 0; i < tests.length; i++) {
        if (tests[i].pending) {
          tests[i].passed = false;
          tests[i].error = 'The test did not finish: ' + tests[i].pending;
        }
      }
      var legacy = G.tests;
      if (legacy && typeof legacy === 'object') {
        for (var k in legacy) {
          if (own(legacy, k) && tests.length < MAX_TESTS) {
            tests.push({ name: k, passed: !!legacy[k], skipped: false, error: null });
          }
        }
      }
      var out = {
        request: null,
        variables: [],
        tests: [],
        console: logs.slice(),
        nextRequest: nextRequest,
        skipRequest: skip,
        visualization: visualization,
      };
      if (dropped) {
        out.console.push({ level: 'warn', message: dropped + ' more console messages were not kept (limit ' + MAX_CONSOLE + ').' });
      }
      var headers = [];
      for (i = 0; i < req.headers.length; i++) {
        headers.push({ key: String(req.headers[i].key), value: stored(req.headers[i].value) });
      }
      out.request = { url: stored(req.url), method: stored(req.method), headers: headers, body: stored(req.body) };
      changes.forEach(function (c) {
        out.variables.push(c);
      });
      for (i = 0; i < tests.length; i++) {
        var t = tests[i];
        out.tests.push({ name: t.name, passed: t.passed, skipped: t.skipped, error: t.error });
      }
      return jsonStringify(out);
    }
    finish.tick = tick;
    return finish;
  };
})()
