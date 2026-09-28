// `require('uuid')`: today's named functions (`uuid.v4()`, `const { v4 } = …`),
// and callable like uuid 3, which Postman shipped (`uuid()` is a v4 UUID).
var uuid = require('uuid');

function v4() {
  return uuid.v4.apply(null, arguments);
}

Object.keys(uuid).forEach(function (key) {
  if (key !== 'default' && key !== '__esModule') v4[key] = uuid[key];
});

module.exports = v4;
