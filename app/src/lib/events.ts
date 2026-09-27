import { listen } from "@tauri-apps/api/event";
import type { StreamEvent } from "../bindings/StreamEvent";
import { isTauri } from "./rpc";

type Handler = (e: StreamEvent) => void;
const handlers = new Set<Handler>();
let started = false;

function dispatch(e: StreamEvent) {
  // The backend batches bursts of events (see BatchingSink); handlers see them one by one, in order.
  if (e.type === "batch") {
    for (const inner of e.events) dispatch(inner);
    return;
  }
  for (const h of handlers) {
    try {
      h(e);
    } catch (err) {
      console.error("event handler failed", err);
    }
  }
}

function start() {
  if (started) return;
  started = true;
  if (isTauri) {
    void listen<StreamEvent>("zv:event", (e) => dispatch(e.payload));
    return;
  }
  // Dev bridge: reconnecting WebSocket.
  const connect = () => {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${proto}://${location.host}/bridge/events`);
    ws.onmessage = (m) => {
      try {
        dispatch(JSON.parse(m.data as string) as StreamEvent);
      } catch {
        /* ignore malformed */
      }
    };
    ws.onclose = () => setTimeout(connect, 1000);
  };
  connect();
}

/** Subscribe to API events; returns an unsubscribe function. */
export function onEvent(handler: Handler): () => void {
  start();
  handlers.add(handler);
  return () => handlers.delete(handler);
}
