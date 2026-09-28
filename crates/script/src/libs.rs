//! The libraries scripts can `require()`: lodash, crypto-js, moment, ajv, … as
//! in Postman's sandbox, and browser versions of some Node modules. They are
//! bundled ahead of time (`vendor/`, see its README) and built into the binary,
//! so nothing is downloaded. Each is compiled to bytecode once per process and
//! loaded into a run only when the script first requires it; the prelude's
//! `require` does the CommonJS part and keeps it for the rest of the run.

use std::ffi::CString;
use std::sync::OnceLock;

use rquickjs::{
    CaughtError, Context, Ctx, Error, Exception, Function, Object, Runtime, TypedArray, Value, WriteOptions, qjs,
};

/// Every name `require` knows, and its CommonJS bundle (`vendor/dist`, same order).
const LIBRARIES: &[(&str, &str)] = &[
    ("ajv", include_str!("../vendor/dist/ajv.js")),
    ("ajv-formats", include_str!("../vendor/dist/ajv-formats.js")),
    ("ajv/dist/2019", include_str!("../vendor/dist/ajv/dist/2019.js")),
    ("ajv/dist/2020", include_str!("../vendor/dist/ajv/dist/2020.js")),
    ("buffer", include_str!("../vendor/dist/buffer.js")),
    ("chai", include_str!("../vendor/dist/chai.js")),
    ("cheerio", include_str!("../vendor/dist/cheerio.js")),
    ("crypto-js", include_str!("../vendor/dist/crypto-js.js")),
    ("csv-parse/lib/sync", include_str!("../vendor/dist/csv-parse/lib/sync.js")),
    ("csv-parse/sync", include_str!("../vendor/dist/csv-parse/sync.js")),
    ("events", include_str!("../vendor/dist/events.js")),
    ("handlebars", include_str!("../vendor/dist/handlebars.js")),
    ("lodash", include_str!("../vendor/dist/lodash.js")),
    ("moment", include_str!("../vendor/dist/moment.js")),
    ("path", include_str!("../vendor/dist/path.js")),
    ("querystring", include_str!("../vendor/dist/querystring.js")),
    ("tv4", include_str!("../vendor/dist/tv4.js")),
    ("url", include_str!("../vendor/dist/url.js")),
    ("util", include_str!("../vendor/dist/util.js")),
    ("uuid", include_str!("../vendor/dist/uuid.js")),
    ("xml2js", include_str!("../vendor/dist/xml2js.js")),
];

/// Node's module wrapper: a bundle evaluates to a function that the prelude's
/// `require` calls with a fresh `module`. Compiled as a script, not a module,
/// so the libraries run in the sloppy mode they were written for.
const WRAP_START: &str = "(function (exports, require, module, __filename, __dirname) {";
const WRAP_END: &str = "\n})";

/// Most bytes one `crypto.getRandomValues` call fills (the Web Crypto limit).
const MAX_RANDOM: usize = 65536;

/// The native side of the prelude's `require` and `crypto` (`setup`'s third
/// argument): `library(name)`, `random(n)` and `names`.
pub(crate) fn host<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Object<'js>> {
    let host = Object::new(ctx.clone())?;
    host.set("library", Function::new(ctx.clone(), library)?)?;
    host.set("random", Function::new(ctx.clone(), random)?)?;
    host.set("names", LIBRARIES.iter().map(|(name, _)| *name).collect::<Vec<_>>())?;
    Ok(host)
}

/// The library's CommonJS factory, or `undefined` for a name Zorvik doesn't have.
fn library<'js>(ctx: Ctx<'js>, name: String) -> rquickjs::Result<Value<'js>> {
    let Some(index) = LIBRARIES.iter().position(|(n, _)| *n == name) else {
        return Ok(Value::new_undefined(ctx));
    };
    let bytecode = bytecode(index).map_err(|message| Exception::throw_message(&ctx, &message))?;
    let raw = ctx.as_raw().as_ptr();
    // SAFETY: the bytecode was written by this same engine (`compile`) and lives
    // for the whole process, as `JS_READ_OBJ_ROM_DATA` needs. On failure both
    // calls leave the exception in the context, and `Error::Exception` hands it
    // on to the script. `JS_EvalFunction` consumes the function; `from_raw` takes
    // ownership of the value it returns.
    unsafe {
        let flags = (qjs::JS_READ_OBJ_BYTECODE | qjs::JS_READ_OBJ_ROM_DATA) as i32;
        let function = qjs::JS_ReadObject(raw, bytecode.as_ptr(), bytecode.len() as _, flags);
        if qjs::JS_IsException(function) {
            return Err(Error::Exception);
        }
        let factory = qjs::JS_EvalFunction(raw, function);
        if qjs::JS_IsException(factory) {
            return Err(Error::Exception);
        }
        Ok(Value::from_raw(ctx, factory))
    }
}

/// `n` cryptographically secure random bytes (rand's thread RNG, seeded by the
/// operating system) for `crypto.getRandomValues`, which uuid and crypto-js use.
fn random<'js>(ctx: Ctx<'js>, n: usize) -> rquickjs::Result<TypedArray<'js, u8>> {
    if n > MAX_RANDOM {
        return Err(Exception::throw_range(&ctx, "crypto.getRandomValues: at most 65536 bytes at a time"));
    }
    let mut bytes = vec![0u8; n];
    rand::fill(&mut bytes[..]);
    TypedArray::new(ctx, bytes)
}

/// The bundle at `index` as bytecode, compiled once per process: loading it is
/// several times faster than parsing the source in every run.
fn bytecode(index: usize) -> Result<&'static [u8], String> {
    static BYTECODE: [OnceLock<Result<Vec<u8>, String>>; LIBRARIES.len()] =
        [const { OnceLock::new() }; LIBRARIES.len()];
    let (name, source) = LIBRARIES[index];
    BYTECODE[index]
        .get_or_init(|| compile(name, source))
        .as_deref()
        .map_err(|e| format!("The built-in library \"{name}\" failed to load: {e}"))
}

/// Bytecode of the wrapped bundle, without function sources (`toString()` of a
/// library function shows no code), which keeps it about a quarter the size.
fn compile(name: &str, source: &str) -> Result<Vec<u8>, String> {
    let code = CString::new(format!("{WRAP_START}{source}{WRAP_END}")).map_err(|e| e.to_string())?;
    let file = CString::new(format!("{name}.js")).map_err(|e| e.to_string())?;
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    context.with(|ctx| {
        let failed = || CaughtError::from_error(&ctx, Error::Exception).to_string();
        let raw = ctx.as_raw().as_ptr();
        // SAFETY: `code` ends with the NUL `JS_Eval` needs. On failure the
        // exception is left in the context; `from_raw` takes ownership of the
        // compiled function, so it is freed with the context.
        let function = unsafe {
            let flags = (qjs::JS_EVAL_TYPE_GLOBAL | qjs::JS_EVAL_FLAG_COMPILE_ONLY) as i32;
            let value = qjs::JS_Eval(raw, code.as_ptr(), code.as_bytes().len() as _, file.as_ptr(), flags);
            if qjs::JS_IsException(value) {
                return Err(failed());
            }
            Value::from_raw(ctx.clone(), value)
        };
        let flags = WriteOptions { strip_source: true, ..Default::default() }.to_flag();
        let mut len = 0;
        // SAFETY: the function is alive; the buffer QuickJS returns is copied, then freed.
        unsafe {
            let buf = qjs::JS_WriteObject(raw, &mut len, function.as_raw(), flags);
            if buf.is_null() {
                return Err(failed());
            }
            let bytes = std::slice::from_raw_parts(buf, len as usize).to_vec();
            qjs::js_free(raw, buf.cast());
            Ok(bytes)
        }
    })
}

#[cfg(test)]
mod tests;
