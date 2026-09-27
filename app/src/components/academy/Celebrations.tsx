// Rewards as they are earned: "+10 XP" for a step, confetti for a lesson, and a moment
// of its own for a new badge, level, finished unit or graduation.
import { useEffect, useRef, useState } from "react";
import { Award, GraduationCap, PartyPopper, Sparkles } from "lucide-react";
import { Dialog as RDialog } from "radix-ui";
import type { Rewards } from "../../bindings/Rewards";
import { dismissCelebration, ensureCourse, openAcademy, useAcademy, type Celebration } from "../../store/academy";
import { Button } from "../ui";
import { allBadges, BadgeArt } from "./parts";

const reducedMotion = () => typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

const isBig = (r: Rewards) => r.badges.length > 0 || r.levelUp != null || r.unitCompleted != null || r.graduated;

export function Celebrations() {
  const queue = useAcademy((s) => s.celebrations);
  const head = queue[0];
  if (!head) return null;
  if (isBig(head.rewards)) return <BigMoment key={head.id} celebration={head} />;
  if (head.rewards.lessonCompleted) return <LessonDone key={head.id} celebration={head} />;
  return <XpChip key={head.id} celebration={head} />;
}

function XpChip({ celebration }: { celebration: Celebration }) {
  const { rewards } = celebration;
  useEffect(() => {
    const t = setTimeout(() => dismissCelebration(celebration.id), rewards.xp ? 1700 : 0);
    return () => clearTimeout(t);
  }, [celebration.id, rewards.xp]);
  if (!rewards.xp) return null;
  return (
    <div className="pointer-events-none fixed left-1/2 top-14 z-[95] -translate-x-1/2" aria-live="polite">
      <div className="zv-rise flex items-center gap-2 rounded-full border border-accent/40 bg-elev px-3.5 py-1.5 text-[13px] font-semibold text-fg shadow-pop" data-testid="xp-chip">
        <Sparkles size={14} className="text-accent" />
        <span className="text-accent">+{rewards.xp} XP</span>
        <span className="font-normal text-muted">{rewards.reasons[0]}</span>
      </div>
    </div>
  );
}

function LessonDone({ celebration }: { celebration: Celebration }) {
  const { rewards } = celebration;
  useEffect(() => {
    const t = setTimeout(() => dismissCelebration(celebration.id), 3200);
    return () => clearTimeout(t);
  }, [celebration.id]);
  return (
    <>
      <Confetti pieces={90} />
      <div className="fixed left-1/2 top-14 z-[95] -translate-x-1/2" aria-live="polite">
        <div className="zv-pop flex items-center gap-3 rounded-2xl border border-success/40 bg-elev px-4 py-3 shadow-pop" data-testid="lesson-complete">
          <PartyPopper size={20} className="text-success" />
          <div>
            <div className="text-[14px] font-semibold text-fg">Lesson complete!</div>
            <div className="text-[12px] text-muted">+{rewards.xp} XP · {rewards.reasons.join(" · ")}</div>
          </div>
          <Button size="sm" variant="ghost" onClick={() => dismissCelebration(celebration.id)}>
            Nice
          </Button>
        </div>
      </div>
    </>
  );
}

function BigMoment({ celebration }: { celebration: Celebration }) {
  const { rewards } = celebration;
  const course = useAcademy((s) => s.course);
  const progress = useAcademy((s) => s.progress);
  useEffect(() => {
    if (!course) void ensureCourse();
  }, [course]);
  const badges = course ? allBadges(course).filter((b) => rewards.badges.includes(b.id)) : [];
  const unit = course?.units.find((u) => u.id === rewards.unitCompleted);
  const title = rewards.graduated
    ? "You graduated!"
    : unit
      ? `${unit.title}: complete!`
      : badges.length
        ? badges.length === 1
          ? "New badge!"
          : `${badges.length} new badges!`
        : `Level ${rewards.levelUp}!`;
  return (
    // A real dialog: focus stays inside while it's open and goes back where it was after.
    <RDialog.Root open onOpenChange={(open) => !open && dismissCelebration(celebration.id)}>
      <RDialog.Portal>
        <RDialog.Overlay className="zv-fade fixed inset-0 z-[96] bg-black/45 backdrop-blur-[2px]" />
        <Confetti pieces={160} />
        <RDialog.Content
          aria-describedby={undefined}
          className="zv-pop fixed left-1/2 top-1/2 z-[97] w-[min(440px,calc(100vw-32px))] -translate-x-1/2 -translate-y-1/2 rounded-3xl border border-line bg-bg px-7 pb-6 pt-7 text-center shadow-pop outline-none"
          data-testid="celebration"
        >
        <div className="text-[11px] font-semibold uppercase tracking-[0.16em] text-accent">{rewards.graduated ? "Zorvik Bootcamp" : "Achievement unlocked"}</div>
        <RDialog.Title className="mt-1 text-[24px] font-semibold tracking-tight text-fg">{title}</RDialog.Title>
        {badges.length > 0 && (
          <div className="mt-5 flex flex-wrap justify-center gap-5">
            {badges.map((b, i) => (
              <div key={b.id} className="flex w-36 flex-col items-center">
                <div className="zv-badge-in" style={{ animationDelay: `${i * 180}ms` }}>
                  <BadgeArt id={b.id} earned size={badges.length > 2 ? 88 : 120} />
                </div>
                <div className="mt-2 text-[14px] font-semibold text-fg">{b.name}</div>
                <div className="mt-0.5 text-[12px] leading-snug text-muted">{b.description}</div>
              </div>
            ))}
          </div>
        )}
        {rewards.levelUp != null && (
          <div className="mx-auto mt-5 flex w-fit items-center gap-2 rounded-full bg-accent-soft px-4 py-1.5 text-[13px] font-semibold text-accent">
            <Award size={15} /> Level {rewards.levelUp}
            {rewards.rankUp && <span className="font-normal text-fg">· you are now a {rewards.rankUp}</span>}
          </div>
        )}
        <div className="mt-4 text-[13px] text-muted">
          +{rewards.xp} XP{progress ? ` · ${progress.xp.toLocaleString()} XP in total` : ""}
        </div>
        <div className="mt-6 flex justify-center gap-2">
          {rewards.graduated && (
            <Button
              variant="primary"
              icon={<GraduationCap size={15} />}
              onClick={() => {
                dismissCelebration(celebration.id);
                void openAcademy({ kind: "badges" });
              }}
            >
              See your certificate
            </Button>
          )}
          <Button variant={rewards.graduated ? "secondary" : "primary"} onClick={() => dismissCelebration(celebration.id)} data-testid="celebration-close">
            Awesome!
          </Button>
        </div>
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  );
}

const CONFETTI_COLORS = ["#D97757", "#E0B03A", "#3A9A5B", "#2E6FB5", "#B8478F", "#2F8F8B", "#F2E6D8"];

/** A short burst of confetti (nothing when the system asks for reduced motion). */
function Confetti({ pieces }: { pieces: number }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [off] = useState(reducedMotion);
  useEffect(() => {
    const el = canvas.current;
    if (!el || off) return;
    const ctx = el.getContext("2d");
    if (!ctx) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = window.innerWidth;
    const h = window.innerHeight;
    el.width = w * dpr;
    el.height = h * dpr;
    ctx.scale(dpr, dpr);
    const parts = Array.from({ length: pieces }, () => ({
      x: w / 2 + (Math.random() - 0.5) * w * 0.3,
      y: h * 0.32,
      vx: (Math.random() - 0.5) * 14,
      vy: -Math.random() * 13 - 4,
      r: Math.random() * Math.PI,
      vr: (Math.random() - 0.5) * 0.3,
      s: Math.random() * 6 + 5,
      c: CONFETTI_COLORS[Math.floor(Math.random() * CONFETTI_COLORS.length)],
    }));
    const start = performance.now();
    let frame = 0;
    const tick = (now: number) => {
      const t = (now - start) / 1000;
      ctx.clearRect(0, 0, w, h);
      ctx.globalAlpha = Math.max(0, 1 - Math.max(0, t - 1.4) / 0.8);
      for (const p of parts) {
        p.vy += 0.42;
        p.vx *= 0.985;
        p.x += p.vx;
        p.y += p.vy;
        p.r += p.vr;
        ctx.save();
        ctx.translate(p.x, p.y);
        ctx.rotate(p.r);
        ctx.fillStyle = p.c;
        ctx.fillRect(-p.s / 2, -p.s / 4, p.s, p.s / 2);
        ctx.restore();
      }
      if (t < 2.2) frame = requestAnimationFrame(tick);
      else ctx.clearRect(0, 0, w, h);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [pieces, off]);
  if (off) return null;
  return <canvas ref={canvas} className="pointer-events-none fixed inset-0 z-[98] h-full w-full" aria-hidden />;
}
