// The graduation certificate: drawn on a canvas, so the preview and the saved PNG match.
import { useEffect, useRef, useState } from "react";
import { Download } from "lucide-react";
import type { CourseView } from "../../bindings/CourseView";
import type { ProgressView } from "../../bindings/ProgressView";
import { pickSavePath } from "../../lib/platform";
import { api, errorMessage } from "../../lib/rpc";
import { toast } from "../../store/toasts";
import { Button } from "../ui";
import { badgeImage } from "./media";

const W = 1600;
const H = 1130;

function loadImage(src: string | undefined): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    if (!src) return resolve(null);
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = src;
  });
}

async function draw(canvas: HTMLCanvasElement, progress: ProgressView, course: CourseView) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  canvas.width = W;
  canvas.height = H;
  const [icon, seal] = await Promise.all([loadImage("/icon.png"), loadImage(badgeImage("graduate"))]);
  const accent = "#B3532F";
  const gold = "#B8912E";
  const ink = "#1F1D1A";
  const soft = "#6B665C";

  // Paper and frames.
  const bg = ctx.createLinearGradient(0, 0, W, H);
  bg.addColorStop(0, "#FDFBF6");
  bg.addColorStop(1, "#F4EEE2");
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, W, H);
  ctx.strokeStyle = gold;
  ctx.lineWidth = 6;
  ctx.strokeRect(40, 40, W - 80, H - 80);
  ctx.strokeStyle = accent;
  ctx.lineWidth = 1.5;
  ctx.strokeRect(58, 58, W - 116, H - 116);

  const center = (text: string, y: number, font: string, color: string) => {
    ctx.font = font;
    ctx.fillStyle = color;
    ctx.textAlign = "center";
    ctx.textBaseline = "alphabetic";
    ctx.fillText(text, W / 2, y);
  };
  const sans = `-apple-system, "Segoe UI", system-ui, Roboto, sans-serif`;
  const serif = `Georgia, "Times New Roman", serif`;

  if (icon) ctx.drawImage(icon, W / 2 - 44, 110, 88, 88);
  center("ZORVIK TRAINING BOOTCAMP", 250, `600 26px ${sans}`, accent);
  center("Certificate of Completion", 330, `italic 40px ${serif}`, soft);
  center("Zorvik Bootcamp Graduate", 450, `700 92px ${serif}`, ink);

  // A thin rule with a diamond.
  ctx.strokeStyle = gold;
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.moveTo(W / 2 - 260, 505);
  ctx.lineTo(W / 2 - 18, 505);
  ctx.moveTo(W / 2 + 18, 505);
  ctx.lineTo(W / 2 + 260, 505);
  ctx.stroke();
  ctx.fillStyle = gold;
  ctx.beginPath();
  ctx.moveTo(W / 2, 493);
  ctx.lineTo(W / 2 + 12, 505);
  ctx.lineTo(W / 2, 517);
  ctx.lineTo(W / 2 - 12, 505);
  ctx.closePath();
  ctx.fill();

  const lessons = course.units.reduce((n, u) => n + u.lessons.length, 0);
  center(`Completed all ${course.units.length} units and ${lessons} hands-on lessons of the Zorvik Training Bootcamp:`, 580, `400 28px ${sans}`, ink);
  center("networking, DNS, HTTP, APIs and auth, TLS, testing, real-time protocols, mocking and load testing.", 624, `400 28px ${sans}`, ink);

  // Stats.
  const stats: [string, string][] = [
    [`Level ${progress.level}`, progress.rank],
    [progress.xp.toLocaleString(), "XP earned"],
    [`${progress.badges.length}`, "badges"],
  ];
  stats.forEach(([big, small], i) => {
    const x = W / 2 + (i - 1) * 300;
    ctx.textAlign = "center";
    ctx.fillStyle = accent;
    ctx.font = `700 40px ${sans}`;
    ctx.fillText(big, x, 740);
    ctx.fillStyle = soft;
    ctx.font = `500 22px ${sans}`;
    ctx.fillText(small, x, 776);
  });

  const date = new Date(progress.graduatedAt ?? Date.now()).toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });
  ctx.textAlign = "left";
  ctx.fillStyle = ink;
  ctx.font = `500 26px ${sans}`;
  ctx.fillText(date, 170, 940);
  ctx.strokeStyle = soft;
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(170, 958);
  ctx.lineTo(560, 958);
  ctx.stroke();
  ctx.fillStyle = soft;
  ctx.font = `400 20px ${sans}`;
  ctx.fillText("Date", 170, 990);

  if (seal) ctx.drawImage(seal, W - 170 - 190, 820, 190, 190);
  center("github.com/LibreGuild/zorvik", H - 88, `400 18px ${sans}`, soft);
}

export function Certificate({ progress, course }: { progress: ProgressView; course: CourseView }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (canvas.current) void draw(canvas.current, progress, course);
  }, [progress, course]);
  const save = async () => {
    const el = canvas.current;
    if (!el) return;
    const path = await pickSavePath("Save your certificate", "Zorvik Bootcamp Graduate.png", [{ name: "PNG image", extensions: ["png"] }]);
    if (!path) return;
    // The name the dialog confirmed is the file written: never a different one.
    if (!path.toLowerCase().endsWith(".png")) {
      toast("error", "Save it as a .png file", "The certificate is a PNG image.");
      return;
    }
    setBusy(true);
    try {
      const png = el.toDataURL("image/png").split(",")[1];
      await api.call("academy.saveCertificate", { path, png });
      toast("success", "Certificate saved", path);
    } catch (e) {
      toast("error", "Could not save the certificate", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="mb-10" data-testid="certificate">
      <div className="mb-3 flex items-center gap-3">
        <h2 className="text-[20px] font-semibold tracking-tight text-fg">Your certificate</h2>
        <Button size="sm" variant="primary" icon={<Download size={13} />} loading={busy} onClick={() => void save()} className="ml-auto">
          Save as image
        </Button>
      </div>
      <canvas ref={canvas} className="w-full rounded-xl border border-line shadow-pop" style={{ aspectRatio: `${W} / ${H}` }} aria-label="Zorvik Bootcamp Graduate certificate" />
    </section>
  );
}
