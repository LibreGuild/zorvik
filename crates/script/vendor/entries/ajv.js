// `require('ajv')`: Ajv 8 (JSON Schema draft-07), with `Ajv2019` and `Ajv2020`
// for the newer drafts and `addFormats` from ajv-formats, all sharing one core.
//
// Postman ships Ajv 6, which ignores keywords it doesn't know and checks the
// standard formats (`date-time`, `email`, `uri`, …) out of the box. Collections
// rely on that, so these classes default to `strict: false` and add the standard
// formats; options passed to the constructor still win.
var Ajv = require('ajv').default;
var Ajv2019 = require('ajv/dist/2019').default;
var Ajv2020 = require('ajv/dist/2020').default;
var addFormats = require('ajv-formats').default;
var formatNames = require('ajv-formats/dist/formats').formatNames;

function compatible(Base) {
  class Compatible extends Base {
    constructor(opts) {
      super(Object.assign({ strict: false }, opts));
      // Formats given in the options win over the standard ones.
      var self = this;
      addFormats(
        this,
        formatNames.filter(function (name) {
          return !(name in self.formats);
        })
      );
    }
  }
  Object.defineProperty(Compatible, 'name', { value: Base.name });
  Compatible.default = Compatible;
  return Compatible;
}

var Draft07 = compatible(Ajv);
Draft07.Ajv = Draft07;
Draft07.Ajv2019 = compatible(Ajv2019);
Draft07.Ajv2020 = compatible(Ajv2020);
Draft07.addFormats = addFormats;

module.exports = Draft07;
