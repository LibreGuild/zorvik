// Node's `timers` for xml2js, which only uses setImmediate with its `async`
// option. Scripts have no timers, so the work runs as a promise job instead.
export function setImmediate(fn) {
  var args = Array.prototype.slice.call(arguments, 1);
  Promise.resolve().then(function () {
    fn.apply(null, args);
  });
}

export function clearImmediate() {}
