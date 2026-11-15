// The protocol document as the SDK emitters see it: named schemas plus the
// methods grouped by who sends them and whether they expect a reply.

import { readFileSync } from "node:fs";
import { join } from "node:path";

export type Schema = boolean | Record<string, any>;

export interface Method {
  name: string;
  params: Schema;
  /** Absent for notifications. */
  result?: Schema;
}

export interface MethodGroup {
  /** Identifier used by emitters, e.g. `HostRequests`. */
  name: string;
  doc: string;
  sender: "host" | "plugin";
  kind: "request" | "notification";
  methods: Method[];
}

export const root = join(import.meta.dir, "../..");
const doc = JSON.parse(readFileSync(join(root, "protocol/openrpc.json"), "utf8"));

export const version: string = doc.info.version;
export const schemas: [string, Record<string, any>][] = Object.entries(doc.components.schemas);

const group = (name: string, text: string, sender: MethodGroup["sender"], kind: MethodGroup["kind"]): MethodGroup => ({
  name,
  doc: text,
  sender,
  kind,
  methods: doc.methods
    .filter((m: any) => m["x-direction"] === (sender === "host" ? "hostToPlugin" : "pluginToHost") && m["x-kind"] === kind)
    .map((m: any) => ({ name: m.name, params: m["x-params"], result: m.result?.schema })),
});

export const groups: MethodGroup[] = [
  group("HostRequests", "Requests the host sends; a plugin answers them.", "host", "request"),
  group("HostNotifications", "Notifications the host sends.", "host", "notification"),
  group("PluginRequests", "Requests a plugin may send to the host.", "plugin", "request"),
  group("PluginNotifications", "Notifications a plugin may send to the host.", "plugin", "notification"),
];

/** The schema name a `$ref` points at. */
export const refName = (schema: Record<string, any>): string => schema.$ref.split("/").pop();
