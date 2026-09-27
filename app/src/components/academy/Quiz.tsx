// Quick-check questions: pick an option, check, see why.
import { useState } from "react";
import { Check, RotateCcw, X } from "lucide-react";
import type { LessonView } from "../../bindings/LessonView";
import type { QuestionResult } from "../../bindings/QuestionResult";
import type { QuizResult } from "../../bindings/QuizResult";
import { errorMessage } from "../../lib/rpc";
import { submitQuiz } from "../../store/academy";
import { toast } from "../../store/toasts";
import { Button, cx } from "../ui";
import { Inline } from "./Markdown";

export function QuizCard({
  index,
  question,
  options,
  value,
  onChange,
  result,
}: {
  index: number;
  question: string;
  options: string[];
  value: number | null;
  onChange: (v: number) => void;
  result: QuestionResult | null;
}) {
  const name = `q-${index}-${question.length}`;
  return (
    <fieldset className="relative rounded-2xl border border-line bg-elev p-4" data-testid={`question-${index}`}>
      <legend className="sr-only">
        Question {index + 1}: {question}
      </legend>
      <div className="mb-3 flex gap-2 text-[14px] font-semibold leading-snug text-fg">
        <span className="text-accent">{index + 1}.</span>
        <span>
          <Inline text={question} />
        </span>
      </div>
      <div className="flex flex-col gap-1.5">
        {options.map((o, i) => {
          const picked = value === i;
          const right = result && result.answer === i;
          const wrong = result && picked && !result.correct;
          return (
            <label
              key={i}
              className={cx(
                "flex items-start gap-2.5 rounded-xl border px-3 py-2 text-[13.5px] transition-colors",
                result ? "cursor-default" : "cursor-pointer hover:border-line-strong hover:bg-panel/60",
                right ? "border-success/60 bg-success/10" : wrong ? "border-danger/50 bg-danger/10" : picked ? "border-accent bg-accent-soft" : "border-line",
              )}
            >
              <input type="radio" name={name} className="mt-[3px] accent-[var(--accent)]" checked={picked} disabled={!!result} onChange={() => onChange(i)} />
              <span className="flex-1 text-fg">
                <Inline text={o} />
              </span>
              {right && <Check size={15} className="mt-0.5 text-success" />}
              {wrong && <X size={15} className="mt-0.5 text-danger" />}
            </label>
          );
        })}
      </div>
      {result && (
        <div className={cx("mt-3 rounded-xl px-3 py-2 text-[13px] leading-relaxed", result.correct ? "bg-success/10 text-fg" : "bg-panel text-fg")}>
          <span className={cx("font-semibold", result.correct ? "text-success" : "text-danger")}>{result.correct ? "Right! " : "Not quite. "}</span>
          <Inline text={result.explain} />
        </div>
      )}
    </fieldset>
  );
}

/** A lesson's quiz. `onPassed` runs when it is passed. */
export function LessonQuiz({ lesson, best, onPassed }: { lesson: LessonView; best: number | null; onPassed?: () => void }) {
  const [answers, setAnswers] = useState<(number | null)[]>(() => lesson.quiz.map(() => null));
  const [result, setResult] = useState<QuizResult | null>(null);
  const [busy, setBusy] = useState(false);
  const total = lesson.quiz.length;
  const submit = async () => {
    setBusy(true);
    try {
      const r = await submitQuiz(lesson.id, answers);
      setResult(r);
      if (r.passed) onPassed?.();
    } catch (e) {
      toast("error", "Could not check the answers", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="mt-12" aria-label="Quick check" data-testid="quiz">
      <div className="mb-4 flex items-baseline gap-3">
        <h2 className="text-[18px] font-semibold tracking-tight text-fg">Quick check</h2>
        {best != null && !result && <span className="text-[12px] text-faint">Best so far: {best} of {total}</span>}
      </div>
      <div className="flex flex-col gap-3">
        {lesson.quiz.map((q, i) => (
          <QuizCard
            key={i}
            index={i}
            question={q.question}
            options={q.options}
            value={answers[i]}
            onChange={(v) => setAnswers((a) => a.map((x, j) => (j === i ? v : x)))}
            result={result?.results[i] ?? null}
          />
        ))}
      </div>
      <div className="mt-4 flex flex-wrap items-center gap-3">
        {!result ? (
          <Button variant="primary" loading={busy} disabled={answers.some((a) => a === null)} onClick={() => void submit()} data-testid="quiz-submit">
            Check my answers
          </Button>
        ) : (
          <>
            <div className={cx("text-[13.5px] font-medium", result.passed ? "text-success" : "text-fg")} data-testid="quiz-score">
              {result.right} of {result.total} right{result.passed ? (result.right === result.total ? ". Perfect!" : ". Nice work!") : ". Read the lesson again and have another go."}
            </div>
            {(!result.passed || result.right < result.total) && (
              <Button
                size="sm"
                icon={<RotateCcw size={13} />}
                onClick={() => {
                  setResult(null);
                  setAnswers(lesson.quiz.map(() => null));
                }}
              >
                Try again
              </Button>
            )}
          </>
        )}
      </div>
    </section>
  );
}
