// Training Bootcamp: the Academy view (course map, lessons, rewards), the running lab and
// the learner's progress. The backend owns progress and checks; rewards arrive as events.
import { create } from "zustand";
import type { AcademyUpdate } from "../bindings/AcademyUpdate";
import type { CourseView } from "../bindings/CourseView";
import type { LabView } from "../bindings/LabView";
import type { LessonView } from "../bindings/LessonView";
import type { ProgressView } from "../bindings/ProgressView";
import type { QuizResult } from "../bindings/QuizResult";
import type { Rewards } from "../bindings/Rewards";
import type { TestOutQuestion } from "../bindings/TestOutQuestion";
import { onEvent } from "../lib/events";
import { api, errorMessage } from "../lib/rpc";
import { confirm } from "./dialogs";
import { refreshRunning, refreshServers } from "./servers";
import { resetTabs, restoreTabs } from "./tabs";
import { toast } from "./toasts";
import { openWorkspace, refreshEnvironments, refreshTree, reloadWorkspace, useWorkspace } from "./workspace";

export type AcademyMode = "workbench" | "academy";
export type AcademyPage = { kind: "home" } | { kind: "lesson"; id: string } | { kind: "badges" } | { kind: "testOut"; unit: string };

/** Something earned, waiting to be celebrated. */
export interface Celebration {
  id: number;
  rewards: Rewards;
}

interface AcademyState {
  mode: AcademyMode;
  page: AcademyPage;
  /** The Bootcamp workspace folder. */
  bootcampPath: string | null;
  course: CourseView | null;
  progress: ProgressView | null;
  lab: LabView | null;
  lessons: Record<string, LessonView>;
  celebrations: Celebration[];
  /** The Lab Guide is folded to a small pill. */
  guideFolded: boolean;
}

const STORAGE_KEY = "zv:academy";

function load(): Partial<Pick<AcademyState, "guideFolded">> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Partial<AcademyState>;
  } catch {
    return {};
  }
}

export const useAcademy = create<AcademyState>(() => ({
  mode: "workbench",
  page: { kind: "home" },
  bootcampPath: null,
  course: null,
  progress: null,
  lab: null,
  lessons: {},
  celebrations: [],
  guideFolded: load().guideFolded ?? false,
}));

useAcademy.subscribe((s) => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ guideFolded: s.guideFolded }));
  } catch {
    /* storage unavailable */
  }
});

const set = useAcademy.setState;
const get = useAcademy.getState;
let celebrationId = 0;

/** Whether the open workspace is the Bootcamp one. */
export function useInBootcamp(): boolean {
  const path = useWorkspace((s) => s.info?.path);
  const bootcamp = useAcademy((s) => s.bootcampPath);
  return !!path && path === bootcamp;
}

export const isBootcampPath = (path: string | null | undefined) => !!path && path === get().bootcampPath;

function apply(update: AcademyUpdate) {
  set((s) => ({
    lab: update.lab,
    progress: update.progress,
    celebrations: update.rewards ? [...s.celebrations, { id: ++celebrationId, rewards: update.rewards }] : s.celebrations,
  }));
}

onEvent((e) => {
  if (e.type === "academy") apply(e.update);
});

/** At startup: the Bootcamp workspace exists, a lab left running is shown again. */
export async function initAcademy() {
  try {
    const [{ path }, lab, progress] = await Promise.all([
      api.call<{ path: string }>("academy.workspace"),
      api.call<LabView | null>("academy.lab"),
      api.call<ProgressView>("academy.progress", { utcOffsetMinutes: -new Date().getTimezoneOffset() }),
    ]);
    set({ bootcampPath: path, lab, progress });
  } catch (e) {
    console.error("academy unavailable", e);
  }
}

export async function ensureCourse() {
  if (get().course) return;
  const [course, progress] = await Promise.all([
    api.call<CourseView>("academy.course"),
    api.call<ProgressView>("academy.progress", { utcOffsetMinutes: -new Date().getTimezoneOffset() }),
  ]);
  set({ course, progress });
}

/** Switch to the Bootcamp workspace (keeping the other one's tabs for later). */
export async function enterBootcamp(): Promise<boolean> {
  let path = get().bootcampPath;
  if (!path) {
    path = (await api.call<{ path: string }>("academy.workspace")).path;
    set({ bootcampPath: path });
  }
  if (useWorkspace.getState().info?.path === path) return true;
  resetTabs();
  try {
    await openWorkspace(path);
  } catch (e) {
    toast("error", "Could not open the Training Bootcamp", errorMessage(e));
  }
  const current = useWorkspace.getState().info?.path;
  if (current) await restoreTabs(current);
  return current === path;
}

export async function setMode(mode: AcademyMode) {
  if (mode === "workbench") {
    set({ mode });
    return;
  }
  if (get().mode === "academy" && isBootcampPath(useWorkspace.getState().info?.path)) return;
  try {
    const [inBootcamp] = await Promise.all([enterBootcamp(), ensureCourse()]);
    if (inBootcamp) set({ mode });
  } catch (e) {
    toast("error", "Could not open the Academy", errorMessage(e));
  }
}

/** Back to the running lab: its workspace, in the workbench. */
export async function backToLab() {
  if (await enterBootcamp()) set({ mode: "workbench", guideFolded: false });
}

// The Academy belongs to the Bootcamp workspace: another workspace (or none) is the workbench.
useWorkspace.subscribe((s, prev) => {
  const path = s.info?.path;
  if (path !== prev.info?.path && !isBootcampPath(path) && get().mode === "academy") set({ mode: "workbench" });
});

export async function openAcademy(page: AcademyPage = get().page) {
  set({ page });
  await setMode("academy");
}

export async function lessonDetail(id: string): Promise<LessonView> {
  const cached = get().lessons[id];
  if (cached) return cached;
  const lesson = await api.call<LessonView>("academy.lesson", { id });
  set((s) => ({ lessons: { ...s.lessons, [id]: lesson } }));
  return lesson;
}

export function openLesson(id: string) {
  set({ page: { kind: "lesson", id } });
  // Remember it for "Continue" (the backend notes it when the lesson is read).
  void api.call("academy.lesson", { id }).catch(() => {});
}

export const goHome = () => set({ page: { kind: "home" } });

// ---- labs ----------------------------------------------------------------------

export async function startLab(id: string) {
  try {
    if (!(await enterBootcamp())) return;
    const lab = await api.call<LabView>("academy.startLab", { id });
    set({ lab, mode: "workbench", guideFolded: false });
    // The lab filled in the Lab environment and made it the active one.
    await Promise.all([reloadWorkspace(), refreshServers(), refreshRunning()]).catch(() => {});
  } catch (e) {
    toast("error", "Could not start the lab", errorMessage(e));
  }
}

export async function stopLab() {
  try {
    await api.call("academy.stopLab");
    set({ lab: null });
    await Promise.all([refreshServers(), refreshRunning()]).catch(() => {});
  } catch (e) {
    toast("error", "Could not stop the lab", errorMessage(e));
  }
}

export async function showHint(step: number) {
  const lab = await api.call<LabView | null>("academy.hint", { step });
  set({ lab });
}

/** Returns whether the answer was right. */
export async function answerStep(step: number, value: string): Promise<boolean> {
  const r = await api.call<{ correct: boolean; lab: LabView | null }>("academy.answer", { step, value });
  set({ lab: r.lab });
  return r.correct;
}

export async function doStep(step: number) {
  try {
    const lab = await api.call<LabView | null>("academy.doStep", { step });
    set({ lab });
    // "Do it for me" may have saved requests, servers or environments.
    await Promise.all([refreshTree(), refreshEnvironments(), refreshServers(), refreshRunning()]).catch(() => {});
  } catch (e) {
    toast("error", "Could not do the step", errorMessage(e));
  }
}

export async function checkNow() {
  const lab = await api.call<LabView | null>("academy.check");
  set({ lab });
}

// ---- quizzes and lessons ---------------------------------------------------------

export function submitQuiz(id: string, answers: (number | null)[]): Promise<QuizResult> {
  return api.call<QuizResult>("academy.quiz", { id, answers });
}

export async function markRead(id: string) {
  await api.call("academy.markRead", { id });
}

export function testOutQuestions(unit: string): Promise<TestOutQuestion[]> {
  return api.call<TestOutQuestion[]>("academy.testOut", { id: unit });
}

export function submitTestOut(unit: string, answers: [string, number][]): Promise<QuizResult> {
  return api.call<QuizResult>("academy.testOutSubmit", { id: unit, answers });
}

export const dismissCelebration = (id: number) => set((s) => ({ celebrations: s.celebrations.filter((c) => c.id !== id) }));

/** Show the Bootcamp workspace again after the backend changed it (reset). */
async function reopenBootcamp(path: string) {
  await openWorkspace(path);
  await restoreTabs(path);
}

export async function resetBootcamp() {
  const ok = await confirm({
    title: "Reset the Training Bootcamp workspace?",
    message: "Its requests, environments and servers are removed and a running lab stops. Your progress, XP and badges stay.",
    confirmLabel: "Reset workspace",
    danger: true,
  });
  if (!ok) return;
  const path = get().bootcampPath;
  const wasOpen = isBootcampPath(useWorkspace.getState().info?.path);
  if (wasOpen) resetTabs();
  try {
    await api.call("academy.reset");
    set({ lab: null });
    toast("success", "Training Bootcamp reset", "Your progress is still here.");
  } catch (e) {
    toast("error", "Could not reset the workspace", errorMessage(e));
  }
  // The same folder, emptied (or untouched when the reset failed): variables, tabs and servers read again.
  if (wasOpen && path) await reopenBootcamp(path).catch((e) => toast("error", "Could not reopen the Training Bootcamp", errorMessage(e)));
  await Promise.all([refreshServers(), refreshRunning()]).catch(() => {});
}
