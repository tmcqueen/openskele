// Settings pane: one generated form per contributed configuration section,
// editable in the user or the workspace scope.

import { h } from "./dom";
import { commands, type ConfigProperty, type Scope, type Value } from "./host";
import { createForm, type JsonSchema } from "./renderers/form";

export interface SettingsPane {
  readonly element: HTMLElement;
  /** Re-reads schemas and values from the host. */
  reload(): Promise<void>;
}

export function createSettings(hasWorkspace: () => boolean): SettingsPane {
  const tabs = h("div", { class: "settings-tabs", role: "tablist" });
  const body = h("div", { class: "settings-body" });
  const element = h("div", { class: "settings" }, tabs, body);
  let scope: Scope = "user";

  /** What a field shows in a scope: its override there, else what it would inherit. */
  function shown(property: ConfigProperty): { value: unknown; modified: boolean } {
    const schema = property.schema as JsonSchema;
    const own = scope === "user" ? property.user : property.workspace;
    const inherited = scope === "user" ? schema.default : (property.user ?? schema.default);
    return { value: own ?? inherited, modified: own != null };
  }

  async function reload(): Promise<void> {
    const sections = await commands.configSections();

    tabs.replaceChildren(
      ...(["user", "workspace"] as const).map((candidate) => {
        const tab = h(
          "button",
          { type: "button", role: "tab", ariaSelected: String(candidate === scope) },
          candidate === "user" ? "User" : "Workspace",
        );
        tab.disabled = candidate === "workspace" && !hasWorkspace();
        tab.title = tab.disabled ? "Open a workspace folder to use workspace settings" : "";
        tab.addEventListener("click", () => {
          scope = candidate;
          void reload();
        });
        return tab;
      }),
    );

    body.replaceChildren(
      ...sections.flatMap((section) => [
        h("h2", {}, section.title),
        createForm({
          fields: section.properties.map((property) => ({
            key: property.key,
            schema: property.schema as JsonSchema,
            ...shown(property),
          })),
          // The host validates, persists and announces the change; the
          // resulting configChanged event is what re-renders this pane.
          change: (key, value) => commands.configSet(key, value as Value, scope),
          reset: (key) => commands.configSet(key, null, scope),
        }),
      ]),
    );
  }

  return { element, reload };
}
