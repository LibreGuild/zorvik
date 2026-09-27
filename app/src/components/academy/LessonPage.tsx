// A lesson: the reading, the lab card beside it, and the quick check.
import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, Check, CheckCircle2, FlaskConical, Play, Square } from "lucide-react";
import type { CourseView } from "../../bindings/CourseView";
import type { LessonView } from "../../bindings/LessonView";
import type { ProgressView } from "../../bindings/ProgressView";
import { errorMessage } from "../../lib/rpc";
import { backToLab, goHome, lessonDetail, markRead, openLesson, startLab, stopLab, useAcademy } from "../../store/academy";
import { toast } from "../../store/toasts";
import { Button, cx, Spinner } from "../ui";
import { Inline, Markdown } from "./Markdown";
import { unitImage } from "./media";
import { LessonMeta, lessonState, unitOf } from "./parts";
import { LessonQuiz } from "./Quiz";

export function LessonPage({ id, course, progress }: { id: string; course: CourseView; progress: ProgressView }) {
  const [lesson, setLesson] = useState<LessonView | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const unit = unitOf(course, id);
  const state = lessonState(progress, id);
  const lab = useAcademy((s) => s.lab);
  const labHere = lab?.lesson === id ? lab : null;
  const vars = useMemo(() => (labHere ? Object.fromEntries(labHere.vars.map((v) => [v.key, v.value])) : undefined), [labHere]);

  useEffect(() => {
    let live = true;
    lessonDetail(id)
      .then((l) => live && setLesson(l))
      .catch((e) => toast("error", "Could not open the lesson", errorMessage(e)));
    scroller.current?.scrollTo({ top: 0 });
    return () => {
      live = false;
    };
  }, [id]);

  if (!lesson || !unit) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner size={18} />
      </div>
    );
  }
  const summary = unit.lessons.find((l) => l.id === id)!;
  const index = unit.lessons.findIndex((l) => l.id === id);
  const done = !!state?.completed;
  const art = index === 0 ? unitImage(unit.image) : undefined;
  return (
    <div ref={scroller} className="relative h-full overflow-y-auto overscroll-contain" data-testid="lesson">
      <div className="sticky top-0 z-10 border-b border-line bg-bg/90 backdrop-blur">
        <div className="mx-auto flex max-w-[1120px] items-center gap-2 px-6 py-2.5">
          <button onClick={goHome} className="rounded-lg px-2 py-1 text-[12.5px] text-muted hover:bg-hover hover:text-fg" data-testid="lesson-back">
            ← Course map
          </button>
          <span className="text-faint">/</span>
          <span className="truncate text-[12.5px]" style={{ color: unit.color }}>
            {unit.title}
          </span>
          <span className="ml-auto flex items-center gap-1">
            <Button size="sm" variant="ghost" icon={<ArrowLeft size={13} />} disabled={!lesson.prev} onClick={() => lesson.prev && openLesson(lesson.prev)}>
              Previous
            </Button>
            <Button size="sm" variant="ghost" disabled={!lesson.next} onClick={() => lesson.next && openLesson(lesson.next)}>
              Next <ArrowRight size={13} />
            </Button>
          </span>
        </div>
      </div>
      <div className="mx-auto grid max-w-[1120px] gap-10 px-6 pb-28 pt-8 lg:grid-cols-[minmax(0,1fr)_300px]">
        <article className="min-w-0 max-w-[700px]">
          <div className="text-[11px] font-semibold uppercase tracking-[0.12em]" style={{ color: unit.color }}>
            Lesson {index + 1} of {unit.lessons.length}
          </div>
          <h1 className="mt-1 text-[28px] font-semibold leading-tight tracking-tight text-fg">{lesson.title}</h1>
          <p className="mt-2 text-[15px] leading-relaxed text-muted">{lesson.summary}</p>
          <div className="mt-3 flex flex-wrap items-center gap-3 text-[12px] text-faint">
            <LessonMeta minutes={summary.minutes} labMinutes={summary.labMinutes} questions={summary.questions} />
            {done && (
              <span className="inline-flex items-center gap-1 font-medium text-success">
                <CheckCircle2 size={13} /> Completed
              </span>
            )}
          </div>
          {art && <img src={art} alt="" draggable={false} className="pointer-events-none mx-auto mt-6 h-48 w-auto select-none object-contain" />}
          <div className="lg:hidden">
            <LabCard lesson={lesson} progress={progress} />
          </div>
          <Markdown source={lesson.body} vars={vars} className="mt-4" />
          {lesson.quiz.length > 0 && <LessonQuiz key={lesson.id} lesson={lesson} best={state?.quizBest ?? null} />}
          {!lesson.lab && lesson.quiz.length === 0 && !done && (
            <Button variant="primary" className="mt-8" icon={<Check size={15} />} onClick={() => void markRead(lesson.id).catch((e) => toast("error", "Could not save", errorMessage(e)))}>
              Mark as done
            </Button>
          )}
          <Footer lesson={lesson} done={done} />
        </article>
        <aside className="hidden lg:block">
          <div className="sticky top-16">
            <LabCard lesson={lesson} progress={progress} />
          </div>
        </aside>
      </div>
    </div>
  );
}

function Footer({ lesson, done }: { lesson: LessonView; done: boolean }) {
  return (
    <div className="mt-12 flex flex-wrap items-center gap-3 border-t border-line pt-6">
      <div className="flex-1 text-[13px] text-muted">
        {done ? "Lesson complete. On to the next one!" : lesson.lab ? "Finish the lab and the quick check to complete this lesson." : "Answer the quick check to complete this lesson."}
      </div>
      {lesson.next ? (
        <Button variant={done ? "primary" : "secondary"} onClick={() => openLesson(lesson.next!)} data-testid="lesson-next">
          Next lesson <ArrowRight size={14} />
        </Button>
      ) : (
        <Button onClick={goHome}>Back to the course map</Button>
      )}
    </div>
  );
}

function LabCard({ lesson, progress }: { lesson: LessonView; progress: ProgressView }) {
  const lab = useAcademy((s) => s.lab);
  const [busy, setBusy] = useState(false);
  if (!lesson.lab) return null;
  const running = lab?.lesson === lesson.id ? lab : null;
  const state = lessonState(progress, lesson.id);
  const start = async () => {
    setBusy(true);
    await startLab(lesson.id);
    setBusy(false);
  };
  return (
    <div className="mt-6 overflow-hidden rounded-2xl border border-line bg-elev shadow-sm lg:mt-0" data-testid="lab-card">
      <div className="border-b border-line/70 px-4 pb-3 pt-4" style={{ background: "linear-gradient(135deg, color-mix(in srgb, var(--accent) 10%, var(--elev)), var(--elev))" }}>
        <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-accent">
          <FlaskConical size={13} /> Lab · {lesson.lab.minutes} min
          <span className="ml-auto rounded-full bg-accent-soft px-2 py-px text-[10.5px] normal-case tracking-normal">+{lesson.lab.steps.length * 10} XP</span>
        </div>
        <div className="mt-1.5 text-[15px] font-semibold text-fg">{lesson.lab.title}</div>
        <div className="mt-1 text-[12.5px] leading-snug text-muted">{lesson.lab.goal}</div>
      </div>
      <ol className="flex flex-col gap-2 px-4 py-3">
        {lesson.lab.steps.map((s, i) => {
          const done = running ? running.steps[i]?.done : false;
          return (
            <li key={i} className="flex gap-2.5 text-[12.5px] leading-snug">
              <span
                className={cx(
                  "mt-px flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[10.5px] font-semibold",
                  done ? "bg-success text-white" : "border border-line-strong text-muted",
                )}
              >
                {done ? <Check size={11} /> : i + 1}
              </span>
              <span className={cx("line-clamp-3", done ? "text-faint line-through" : "text-muted")}>
                <Inline text={s.split("\n")[0]} />
              </span>
            </li>
          );
        })}
      </ol>
      <div className="flex flex-col gap-2 border-t border-line/70 px-4 py-3">
        {running ? (
          <>
            <Button variant="primary" icon={<Play size={14} />} onClick={() => void backToLab()} data-testid="lab-continue">
              {running.finished ? "Back to the lab" : "Continue the lab"}
            </Button>
            <Button variant="ghost" size="sm" icon={<Square size={12} />} onClick={() => void stopLab()}>
              Stop lab
            </Button>
          </>
        ) : (
          <Button variant="primary" icon={<Play size={14} />} loading={busy} onClick={() => void start()} data-testid="lab-start">
            {state?.labDone ? "Practice again" : "Start lab"}
          </Button>
        )}
        {state?.labDone && !running && <div className="text-center text-[11.5px] text-success">Lab done ✓</div>}
      </div>
    </div>
  );
}
