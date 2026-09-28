// The few `process` fields libraries read (util's debuglog, path.resolve). Only
// the bundles see it: scripts have no `process`.
export var process = {
  env: {},
  argv: [],
  version: '',
  versions: {},
  platform: 'browser',
  browser: true,
  cwd: function () {
    return '/';
  },
  nextTick: function (fn) {
    var args = Array.prototype.slice.call(arguments, 1);
    Promise.resolve().then(function () {
      fn.apply(null, args);
    });
  },
};
