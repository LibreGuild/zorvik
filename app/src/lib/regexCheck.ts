// Whether a regular expression works with the backend's engine (Rust `regex`: no look-around
// or backreferences), which is what rules, waits and captures run on. Checked a moment after
// typing stops; answers are kept per pattern.
import { useEffect, useState } from "react";
import { api, errorMessage } from "./rpc";

const NO_FLAGS = { ignoreCase: false, multiLine: false, dotAll: false, ignoreWhitespace: false };
const known = new Map<string, string | null>();

/** The problem with `pattern` ("invalid regex: …"), null when it works or isn't checked yet. */
export function useRegexProblem(pattern: string | null): string | null {
  const [problem, setProblem] = useState<string | null>(() => (pattern === null ? null : (known.get(pattern) ?? null)));
  useEffect(() => {
    if (pattern === null) {
      setProblem(null);
      return;
    }
    if (known.has(pattern)) {
      setProblem(known.get(pattern)!);
      return;
    }
    let current = true;
    const timer = setTimeout(() => {
      api
        .regexTest(pattern, NO_FLAGS, "")
        .then(() => null)
        .catch((e) => `invalid regex: ${errorMessage(e)}`)
        .then((result) => {
          if (known.size > 500) known.clear();
          known.set(pattern, result);
          if (current) setProblem(result);
        });
    }, 300);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [pattern]);
  return problem;
}
