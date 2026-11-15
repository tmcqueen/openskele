import { h } from "./dom";

export type ToastLevel = "info" | "warning" | "error";

const DISMISS_AFTER_MS = 6000;

export function toast(level: ToastLevel, message: string, source?: string | null): void {
  const element = h("div", { class: `toast ${level}` }, source && h("strong", {}, `${source}: `), message);
  document.getElementById("toasts")!.append(element);
  setTimeout(() => element.remove(), DISMISS_AFTER_MS);
}
