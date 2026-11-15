// The webview's only link to the workbench: typed commands out, one event
// stream in. No workbench state is kept here beyond what is on screen.

import { Channel } from "@tauri-apps/api/core";

import { commands, type HostEvent, type Value } from "./bindings";
import { toast } from "./toast";

export { commands };
export type * from "./bindings";

type Listener = (event: HostEvent) => void;
const listeners = new Set<Listener>();

export function onHostEvent(listener: Listener): void {
  listeners.add(listener);
}

/** Starts the event stream. Call once, after listeners are registered. */
export function connect(): Promise<void> {
  const channel = new Channel<HostEvent>();
  channel.onmessage = (event) => listeners.forEach((listener) => listener(event));
  return commands.subscribe(channel);
}

/** Runs a command, surfacing a failure to the user instead of the caller. */
export async function run(command: string, args: Value[] = []): Promise<void> {
  try {
    await commands.executeCommand(command, args);
  } catch (error) {
    toast("error", String(error));
  }
}
