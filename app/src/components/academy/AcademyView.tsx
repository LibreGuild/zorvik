// The Academy: the Training Bootcamp's own interface (course map, lessons, badges), switched
// with Workbench in the title bar. Loaded on demand.
import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowRight, Award, BookOpen, Check, ChevronDown, Flame, FlaskConical, GraduationCap, MoreHorizontal, RotateCcw, Sparkles, Trophy } from "lucide-react";
import type { CourseView } from "../../bindings/CourseView";
import type { ProgressView } from "../../bindings/ProgressView";
import type { QuizResult } from "../../bindings/QuizResult";
import type { TestOutQuestion } from "../../bindings/TestOutQuestion";
import type { UnitView } from "../../bindings/UnitView";
import { errorMessage } from "../../lib/rpc";
import { backToLab, goHome, openAcademy, openLesson, resetBootcamp, setMode, submitTestOut, testOutQuestions, useAcademy } from "../../store/academy";
import { toast } from "../../store/toasts";
import { Button, cx, Menu, Spinner } from "../ui";
import { Certificate } from "./Certificate";
import { LessonPage } from "./LessonPage";
import { unitImage } from "./media";
import { allBadges, BadgeArt, Bar, hasBadge, isComplete, LessonMeta, LevelRing, nextLesson, unitDone } from "./parts";
import { QuizCard } from "./Quiz";

export default function AcademyView() {
  const course = useAcademy((s) => s.course);
  const progress = useAcademy((s) => s.progress);
  const page = useAcademy((s) => s.page);
  if (!course || !progress) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner size={20} />
      </div>
    );
  }
  return (
    <div className="relative h-full bg-bg" data-testid="academy">
      {page.kind === "lesson" ? (
        <LessonPage key={page.id} id={page.id} course={course} progress={progress} />
      ) : page.kind === "badges" ? (
        <BadgesPage course={course} progress={progress} />
      ) : page.kind === "testOut" ? (
        <TestOutPage key={page.unit} unit={course.units.find((u) => u.id === page.unit)!} progress={progress} />
      ) : (
        <Home course={course} progress={progress} />
      )}
    </div>
  );
}

function Scroller({ children }: { children: React.ReactNode }) {
  return <div className="relative h-full overflow-y-auto overscroll-contain">{children}</div>;
}

// ---- home -----------------------------------------------------------------------

function Header({ progress, course }: { progress: ProgressView; course: CourseView }) {
  const badges = progress.badges.length;
  const total = course.units.length + course.extraBadges.length;
  return (
    <header
      className="relative overflow-hidden border-b border-line"
      style={{ background: "linear-gradient(120deg, color-mix(in srgb, var(--accent) 16%, var(--panel)) 0%, var(--panel) 55%, color-mix(in srgb, var(--info) 10%, var(--panel)) 100%)" }}
    >
      <div className="mx-auto flex max-w-[1080px] flex-wrap items-center gap-5 px-8 py-6">
        <LevelRing progress={progress} size={72} />
        <div className="min-w-[220px] flex-1">
          <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-accent">Training Bootcamp</div>
          <div className="mt-0.5 text-[22px] font-semibold tracking-tight text-fg" data-testid="academy-rank">
            {progress.rank}
          </div>
          <div className="mt-2 flex max-w-[360px] items-center gap-2.5">
            <Bar value={(progress.xp - progress.levelXp) / Math.max(1, progress.nextLevelXp - progress.levelXp)} className="h-2 flex-1" />
            <span className="whitespace-nowrap text-[12px] tabular-nums text-muted" data-testid="academy-xp">
              {progress.xp.toLocaleString()} / {progress.nextLevelXp.toLocaleString()} XP
            </span>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Stat
            icon={<Flame size={15} className={progress.activeToday ? "text-[#e0612f]" : "text-faint"} />}
            value={progress.streak}
            label={progress.activeToday || progress.streak === 0 ? "day streak" : "day streak · learn today to keep it"}
          />
          <Stat icon={<BookOpen size={15} className="text-info" />} value={`${progress.completedLessons}/${progress.totalLessons}`} label="lessons" />
          <button onClick={() => void openAcademy({ kind: "badges" })} className="rounded-xl text-left outline-none hover:brightness-105 focus-visible:ring-2 focus-visible:ring-accent" data-testid="academy-badges">
            <Stat icon={<Award size={15} className="text-warning" />} value={`${badges}/${total}`} label="badges" />
          </button>
          <Menu
            align="end"
            trigger={
              <button aria-label="Academy menu" className="flex h-9 w-9 items-center justify-center rounded-xl text-muted hover:bg-hover hover:text-fg">
                <MoreHorizontal size={17} />
              </button>
            }
            entries={[
              { label: "Badges and certificate", icon: <Trophy size={14} />, onSelect: () => void openAcademy({ kind: "badges" }) },
              { label: "Open the workbench", icon: <FlaskConical size={14} />, onSelect: () => void setMode("workbench") },
              { separator: true },
              { label: "Reset Bootcamp workspace…", icon: <RotateCcw size={14} />, onSelect: () => void resetBootcamp() },
            ]}
          />
        </div>
      </div>
    </header>
  );
}

function Stat({ icon, value, label }: { icon: React.ReactNode; value: React.ReactNode; label: string }) {
  return (
    <div className="flex items-center gap-2 rounded-xl border border-line bg-elev/80 px-3 py-2 shadow-sm">
      {icon}
      <div className="leading-tight">
        <div className="text-[14px] font-semibold tabular-nums text-fg">{value}</div>
        <div className="text-[11px] text-faint">{label}</div>
      </div>
    </div>
  );
}

function Home({ course, progress }: { course: CourseView; progress: ProgressView }) {
  const lab = useAcademy((s) => s.lab);
  const next = useMemo(() => nextLesson(course, progress), [course, progress]);
  // The unit to continue starts open.
  const [open, setOpen] = useState<Set<string>>(() => new Set(next ? [next.unit.id] : [course.units[0]?.id]));
  const toggle = (id: string) =>
    setOpen((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  return (
    <Scroller>
      <Header progress={progress} course={course} />
      <div className="mx-auto max-w-[1080px] px-8 pb-24 pt-6">
        {lab && !lab.finished && (
          <div className="mb-4 flex flex-wrap items-center gap-3 rounded-xl border border-accent/40 bg-accent-soft px-4 py-3">
            <FlaskConical size={16} className="text-accent" />
            <div className="min-w-0 flex-1 text-[13px] text-fg">
              Lab in progress: <span className="font-semibold">{lab.title}</span> ({lab.steps.filter((s) => s.done).length}/{lab.steps.length} steps)
            </div>
            <Button size="sm" variant="primary" onClick={() => void backToLab()}>
              Back to the lab
            </Button>
          </div>
        )}
        {progress.graduatedAt ? <Graduated /> : next && <ContinueCard unit={next.unit} lessonId={next.lesson.id} started={progress.completedLessons > 0 || !!progress.lastLesson} />}
        <h2 className="mb-3 mt-10 text-[13px] font-semibold uppercase tracking-[0.12em] text-faint">Course map</h2>
        <ol className="relative flex flex-col gap-3" data-testid="course-map">
          {course.units.map((unit, i) => (
            <UnitCard key={unit.id} unit={unit} index={i} progress={progress} open={open.has(unit.id)} onToggle={() => toggle(unit.id)} />
          ))}
        </ol>
      </div>
    </Scroller>
  );
}

function ContinueCard({ unit, lessonId, started }: { unit: UnitView; lessonId: string; started: boolean }) {
  const lesson = unit.lessons.find((l) => l.id === lessonId)!;
  const art = unitImage(unit.image);
  return (
    <div className="relative overflow-hidden rounded-2xl border border-line bg-elev shadow-sm" style={{ borderColor: `color-mix(in srgb, ${unit.color} 35%, var(--border))` }}>
      <div className="absolute inset-y-0 left-0 w-1.5" style={{ background: unit.color }} />
      <div className="flex flex-wrap items-center gap-6 py-5 pl-7 pr-6">
        <div className="min-w-[260px] flex-1">
          <div className="text-[11px] font-semibold uppercase tracking-[0.12em]" style={{ color: unit.color }}>
            {started ? "Continue" : "Start here"} · {unit.title}
          </div>
          <div className="mt-1 text-[20px] font-semibold tracking-tight text-fg">{lesson.title}</div>
          <p className="mt-1 max-w-[520px] text-[13.5px] leading-relaxed text-muted">{lesson.summary}</p>
          <div className="mt-3 flex flex-wrap items-center gap-3 text-[12px] text-faint">
            <LessonMeta minutes={lesson.minutes} labMinutes={lesson.labMinutes} questions={lesson.questions} />
          </div>
          <Button variant="primary" className="mt-4" icon={<ArrowRight size={15} />} onClick={() => openLesson(lesson.id)} data-testid="academy-continue">
            {started ? "Continue" : "Start the Bootcamp"}
          </Button>
        </div>
        {art && <img src={art} alt="" draggable={false} className="pointer-events-none h-36 w-auto select-none object-contain" />}
      </div>
    </div>
  );
}

function Graduated() {
  return (
    <div className="flex flex-wrap items-center gap-5 rounded-2xl border border-line bg-elev px-6 py-5 shadow-sm">
      <BadgeArt id="graduate" earned size={72} className="zv-badge-in" />
      <div className="min-w-[240px] flex-1">
        <div className="text-[18px] font-semibold text-fg">You graduated from the Bootcamp!</div>
        <div className="mt-1 text-[13px] text-muted">Your certificate is ready. Keep practising: every lab can be done again.</div>
      </div>
      <Button variant="primary" icon={<GraduationCap size={15} />} onClick={() => void openAcademy({ kind: "badges" })}>
        View certificate
      </Button>
    </div>
  );
}

function UnitCard({ unit, index, progress, open, onToggle }: { unit: UnitView; index: number; progress: ProgressView; open: boolean; onToggle: () => void }) {
  const done = unitDone(progress, unit);
  const total = unit.lessons.length;
  const complete = progress.units.includes(unit.id);
  const art = unitImage(unit.image);
  return (
    <li className="overflow-hidden rounded-2xl border border-line bg-elev shadow-sm" data-testid={`unit-${unit.id}`}>
      <button onClick={onToggle} aria-expanded={open} className="flex w-full items-center gap-4 px-4 py-3 text-left outline-none hover:bg-panel/60 focus-visible:bg-panel/60">
        <div className="flex h-14 w-20 shrink-0 items-center justify-center overflow-hidden rounded-xl" style={{ background: `color-mix(in srgb, ${unit.color} 12%, var(--panel))` }}>
          {art && <img src={art} alt="" loading="lazy" draggable={false} className="h-full w-full select-none object-contain p-1" />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-[11px] font-semibold tabular-nums" style={{ color: unit.color }}>
              {unit.capstone ? "CAPSTONE" : `UNIT ${index}`}
            </span>
            {complete && (
              <span className="inline-flex items-center gap-1 rounded-full bg-success/15 px-1.5 py-px text-[10.5px] font-semibold text-success">
                <Check size={11} /> Done
              </span>
            )}
          </div>
          <div className="truncate text-[15px] font-semibold text-fg">{unit.title}</div>
          <div className="truncate text-[12.5px] text-muted">{unit.summary}</div>
        </div>
        <div className="hidden w-40 shrink-0 flex-col gap-1.5 sm:flex">
          <div className="text-right text-[11.5px] tabular-nums text-faint">
            {done}/{total} lessons
          </div>
          <Bar value={total ? done / total : 0} color={unit.color} />
        </div>
        <BadgeArt id={unit.badge.id} earned={hasBadge(progress, unit.badge.id)} size={48} />
        <ChevronDown size={16} className={cx("shrink-0 text-faint transition-transform", open && "rotate-180")} />
      </button>
      {open && (
        <div className="border-t border-line/70 px-3 pb-3 pt-2">
          <ol className="flex flex-col">
            {unit.lessons.map((l, i) => {
              const finished = isComplete(progress, l.id);
              return (
                <li key={l.id}>
                  <button
                    onClick={() => openLesson(l.id)}
                    className="group flex w-full items-center gap-3 rounded-xl px-2 py-2 text-left outline-none hover:bg-hover focus-visible:bg-hover"
                    data-testid={`lesson-${l.id}`}
                  >
                    <span
                      className={cx(
                        "flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-[12px] font-semibold tabular-nums",
                        finished ? "text-white" : "border border-line-strong text-muted",
                      )}
                      style={finished ? { background: unit.color } : undefined}
                    >
                      {finished ? <Check size={14} /> : i + 1}
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[13.5px] font-medium text-fg">{l.title}</div>
                      <div className="truncate text-[12px] text-faint">{l.summary}</div>
                    </div>
                    <div className="hidden shrink-0 items-center gap-3 text-[11.5px] text-faint md:flex">
                      <LessonMeta minutes={l.minutes} labMinutes={l.labMinutes} questions={0} />
                    </div>
                    <ArrowRight size={14} className="shrink-0 text-faint opacity-0 transition-opacity group-hover:opacity-100" />
                  </button>
                </li>
              );
            })}
          </ol>
          {!unit.capstone && !progress.units.includes(unit.id) && unit.lessons.some((l) => l.questions > 0) && (
            <div className="mt-2 flex items-center gap-3 rounded-xl bg-panel/70 px-3 py-2 text-[12.5px] text-muted">
              <Sparkles size={14} className="shrink-0 text-accent" />
              <span className="flex-1">Know this already? Pass a short quiz to earn the {unit.badge.name} badge.</span>
              <Button size="sm" onClick={() => void openAcademy({ kind: "testOut", unit: unit.id })}>
                Test out
              </Button>
            </div>
          )}
        </div>
      )}
    </li>
  );
}

// ---- badges ---------------------------------------------------------------------

function BackBar({ title }: { title: string }) {
  return (
    <div className="sticky top-0 z-10 border-b border-line bg-bg/90 backdrop-blur">
      <div className="mx-auto flex max-w-[1080px] items-center gap-2 px-8 py-2.5">
        <button onClick={goHome} className="rounded-lg px-2 py-1 text-[12.5px] text-muted hover:bg-hover hover:text-fg">
          ← Course map
        </button>
        <span className="text-faint">/</span>
        <span className="text-[12.5px] font-medium text-fg">{title}</span>
      </div>
    </div>
  );
}

function BadgesPage({ course, progress }: { course: CourseView; progress: ProgressView }) {
  const badges = allBadges(course);
  return (
    <Scroller>
      <BackBar title="Badges and certificate" />
      <div className="mx-auto max-w-[1080px] px-8 pb-24 pt-6">
        {progress.graduatedAt && <Certificate progress={progress} course={course} />}
        <h2 className="mb-4 mt-2 text-[20px] font-semibold tracking-tight text-fg">
          Badges <span className="text-[14px] font-normal text-faint">{progress.badges.length} of {badges.length}</span>
        </h2>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
          {badges.map((b) => {
            const earned = progress.badges.find((x) => x.id === b.id);
            return (
              <div key={b.id} className={cx("flex flex-col items-center rounded-2xl border border-line bg-elev p-4 text-center", !earned && "bg-panel/50")} data-testid={`badge-${b.id}`}>
                <BadgeArt id={b.id} earned={!!earned} size={96} />
                <div className={cx("mt-3 text-[13.5px] font-semibold", earned ? "text-fg" : "text-muted")}>{b.name}</div>
                <div className="mt-1 text-[12px] leading-snug text-faint">{b.description}</div>
                <div className="mt-2 text-[11px] font-medium" style={{ color: earned ? "var(--success)" : "var(--faint)" }}>
                  {earned ? `Earned ${new Date(earned.at).toLocaleDateString()}` : b.unit ? `Finish “${b.unit.title}”` : "Locked"}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </Scroller>
  );
}

// ---- test out -------------------------------------------------------------------

function TestOutPage({ unit, progress }: { unit: UnitView; progress: ProgressView }) {
  const [questions, setQuestions] = useState<TestOutQuestion[] | null>(null);
  const [answers, setAnswers] = useState<Record<string, number>>({});
  const [result, setResult] = useState<QuizResult | null>(null);
  const [busy, setBusy] = useState(false);
  const top = useRef<HTMLDivElement>(null);
  useEffect(() => {
    testOutQuestions(unit.id)
      .then(setQuestions)
      .catch((e) => toast("error", "Could not load the quiz", errorMessage(e)));
  }, [unit.id]);
  const submit = async () => {
    if (!questions) return;
    setBusy(true);
    try {
      setResult(await submitTestOut(unit.id, Object.entries(answers)));
      top.current?.scrollIntoView({ behavior: "smooth" });
    } catch (e) {
      toast("error", "Could not check the answers", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const earned = progress.units.includes(unit.id);
  return (
    <Scroller>
      <BackBar title={`Test out: ${unit.title}`} />
      <div ref={top} className="mx-auto max-w-[720px] px-8 pb-24 pt-6">
        <div className="flex items-center gap-4">
          <BadgeArt id={unit.badge.id} earned={earned} size={72} />
          <div>
            <h1 className="text-[22px] font-semibold tracking-tight text-fg">Test out of “{unit.title}”</h1>
            <p className="mt-1 text-[13.5px] text-muted">Get 80% right to earn the {unit.badge.name} badge and the unit's XP. The lessons stay open to read and practise.</p>
          </div>
        </div>
        {result && (
          <div className={cx("mt-5 rounded-xl border px-4 py-3 text-[13.5px]", result.passed ? "border-success/40 bg-success/10 text-fg" : "border-warning/40 bg-warning/10 text-fg")}>
            {result.passed
              ? `Passed: ${result.right} of ${result.total}. The badge is yours!`
              : `${result.right} of ${result.total}: you need ${Math.ceil(result.total * 0.8)} to pass. Read the unit's lessons, then try again.`}
          </div>
        )}
        {!questions ? (
          <div className="mt-10 flex justify-center">
            <Spinner />
          </div>
        ) : (
          <div className="mt-6 flex flex-col gap-4">
            {questions.map((q, i) => (
              <QuizCard
                key={q.id}
                index={i}
                question={q.question}
                options={q.options}
                value={answers[q.id] ?? null}
                onChange={(v) => setAnswers((a) => ({ ...a, [q.id]: v }))}
                result={result?.results[i] ?? null}
              />
            ))}
            <div className="flex gap-2">
              {result && !result.passed ? (
                <Button
                  onClick={() => {
                    setResult(null);
                    setAnswers({});
                  }}
                >
                  Try again
                </Button>
              ) : (
                !result && (
                  <Button variant="primary" loading={busy} disabled={Object.keys(answers).length < questions.length} onClick={() => void submit()}>
                    Check my answers
                  </Button>
                )
              )}
              {result?.passed && (
                <Button variant="primary" onClick={goHome}>
                  Back to the course map
                </Button>
              )}
            </div>
          </div>
        )}
      </div>
    </Scroller>
  );
}
