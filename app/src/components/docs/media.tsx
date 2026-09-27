// Docs illustrations (assets/docs/*.webp, generated for Zorvik) and topic icons.
import type { ReactNode } from "react";
import { Activity, Bot, Boxes, Braces, FolderGit2, Import, KeyRound, ListChecks, Radio, Send, Server, Terminal, TestTubeDiagonal, Variable, Wrench } from "lucide-react";

const images = import.meta.glob("../../assets/docs/*.webp", { eager: true, query: "?url", import: "default" }) as Record<string, string>;

export const docImage = (name: string): string | undefined => images[`../../assets/docs/${name}.webp`];

export const TOPIC_ICONS: Record<string, (size: number) => ReactNode> = {
  requests: (s) => <Send size={s} />,
  graphql: (s) => <Braces size={s} />,
  grpc: (s) => <Boxes size={s} />,
  realtime: (s) => <Radio size={s} />,
  variables: (s) => <Variable size={s} />,
  auth: (s) => <KeyRound size={s} />,
  scripts: (s) => <TestTubeDiagonal size={s} />,
  runner: (s) => <ListChecks size={s} />,
  load: (s) => <Activity size={s} />,
  servers: (s) => <Server size={s} />,
  tools: (s) => <Wrench size={s} />,
  import: (s) => <Import size={s} />,
  workspace: (s) => <FolderGit2 size={s} />,
  cli: (s) => <Terminal size={s} />,
  agents: (s) => <Bot size={s} />,
};
