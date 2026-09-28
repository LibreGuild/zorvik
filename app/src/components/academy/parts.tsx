// Small pieces the Academy screens share: the level ring, badge art, progress bars.
import type { CourseView } from "../../bindings/CourseView";
import type { LessonSummary } from "../../bindings/LessonSummary";
import type { ProgressView } from "../../bindings/ProgressView";
import type { UnitView } from "../../bindings/UnitView";
import { Clock, FlaskConical, Sparkles } from "lucide-react";
import { cx } from "../ui";
import { badgeImage } from "./media";

/** XP through the current level, 0–1. */
export function levelFraction(p: ProgressView): number {
  const span = p.nextLevelXp - p.levelXp;
  return span > 0 ? Math.min(1, Math.max(0, (p.xp - p.levelXp) / span)) : 1;
}

export function LevelRing({ progress, size = 64 }: { progress: ProgressView; size?: number }) {
  const stroke = Math.max(4, size / 12);
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const f = levelFraction(progress);
  return (
    <div className="relative shrink-0" style={{ width: size, height: size }} aria-label={`Level ${progress.level}`}>
      <svg width={size} height={size} className="-rotate-90">
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--border-strong)" strokeWidth={stroke} opacity={0.5} />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke="var(--accent)"
          strokeWidth={stroke}
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={c * (1 - f)}
          style={{ transition: "stroke-dashoffset 0.8s cubic-bezier(0.2, 0.8, 0.2, 1)" }}
        />
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center leading-none">
        <span className="text-[9px] font-semibold uppercase tracking-wide text-faint">Lvl</span>
        <span className="font-semibold tabular-nums text-fg" style={{ fontSize: size * 0.32 }}>
          {progress.level}
        </span>
      </div>
    </div>
  );
}

export function BadgeArt({ id, earned, size = 56, className }: { id: string; earned: boolean; size?: number; className?: string }) {
  const src = badgeImage(id);
  return (
    <div className={cx("relative shrink-0", className)} style={{ width: size, height: size }}>
      {src ? (
        <img
          src={src}
          alt=""
          draggable={false}
          loading="lazy"
          className={cx("h-full w-full select-none object-contain transition-[filter,opacity]", !earned && "opacity-45 grayscale")}
        />
      ) : (
        <div className="h-full w-full rounded-full bg-panel-2" />
      )}
    </div>
  );
}

export function Bar({ value, color = "var(--accent)", className }: { value: number; color?: string; className?: string }) {
  return (
    <div className={cx("h-1.5 overflow-hidden rounded-full bg-panel-2", className)}>
      <div className="h-full rounded-full transition-[width] duration-700" style={{ width: `${Math.round(Math.min(1, Math.max(0, value)) * 100)}%`, background: color }} />
    </div>
  );
}

export const isComplete = (p: ProgressView | null, lesson: string) => !!p?.lessons.find((l) => l.id === lesson)?.completed;
/** Added by an update and not opened yet. */
export const isNew = (p: ProgressView | null, lesson: string) => !!p?.lessons.find((l) => l.id === lesson)?.new;

export function NewChip() {
  return <span className="inline-flex shrink-0 items-center rounded-full bg-accent/15 px-1.5 py-px text-[10.5px] font-semibold text-accent">New</span>;
}
export const lessonState = (p: ProgressView | null, lesson: string) => p?.lessons.find((l) => l.id === lesson) ?? null;
export const hasBadge = (p: ProgressView | null, id: string) => !!p?.badges.some((b) => b.id === id);

export function unitDone(p: ProgressView | null, unit: UnitView) {
  return unit.lessons.filter((l) => isComplete(p, l.id)).length;
}

/** The lesson to continue with: the last one opened when unfinished, else the first unfinished one. */
export function nextLesson(course: CourseView, p: ProgressView | null): { unit: UnitView; lesson: LessonSummary } | null {
  const all = course.units.flatMap((unit) => unit.lessons.map((lesson) => ({ unit, lesson })));
  const last = p?.lastLesson ? all.find((x) => x.lesson.id === p.lastLesson) : undefined;
  if (last && !isComplete(p, last.lesson.id)) return last;
  const after = last ? all.slice(all.indexOf(last) + 1) : all;
  return after.find((x) => !isComplete(p, x.lesson.id)) ?? all.find((x) => !isComplete(p, x.lesson.id)) ?? null;
}

export function unitOf(course: CourseView | null, lesson: string): UnitView | undefined {
  return course?.units.find((u) => u.lessons.some((l) => l.id === lesson));
}

/** Every badge: the units' then the extra ones. */
export function allBadges(course: CourseView) {
  return [...course.units.map((u) => ({ ...u.badge, unit: u })), ...course.extraBadges.map((b) => ({ ...b, unit: null as UnitView | null }))];
}

export function LessonMeta({ minutes, labMinutes, questions }: { minutes: number; labMinutes: number | null; questions: number }) {
  return (
    <>
      <span className="inline-flex items-center gap-1">
        <Clock size={12} /> {minutes} min read
      </span>
      {labMinutes != null && (
        <span className="inline-flex items-center gap-1">
          <FlaskConical size={12} /> Lab · {labMinutes} min
        </span>
      )}
      {questions > 0 && (
        <span className="inline-flex items-center gap-1">
          <Sparkles size={12} /> {questions} questions
        </span>
      )}
    </>
  );
}
