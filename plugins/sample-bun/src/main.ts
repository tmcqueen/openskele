// Sample plugin: one command that greets using its settings, and a tree view
// of what it has said so far. Everything it shows is data; the host renders it.

import { join } from "node:path";

import { runPlugin, type TreeItem } from "@openskele/sdk";

const VIEW = "hello.greetings";
const SETTINGS = ["hello.name", "hello.greeting", "hello.repeat", "hello.shout"];

runPlugin({
  name: "Hello",
  version: "0.1.0",

  activate(host) {
    const greetings: string[] = [];
    // Deliberately on stdout, the protocol stream: the host passes stray
    // lines like this to the plugin's output channel instead of failing.
    console.log("sample.hello stdout check");

    host.commands.register("hello.greet", async (name) => {
      const [fallback, greeting, repeat, shout] = await Promise.all([
        host.config.get<string>("hello.name"),
        host.config.get<string>("hello.greeting"),
        host.config.get<number>("hello.repeat"),
        host.config.get<boolean>("hello.shout"),
      ]);
      const who = typeof name === "string" && name ? name : fallback;
      const text = `${greeting}, ${who}!`;
      const message = shout ? text.toUpperCase() : text;

      for (let i = 0; i < repeat; i++) {
        greetings.push(message);
        host.terminal.write(`${message}\r\n`);
      }
      host.window.showMessage(message);
      host.views.refresh(VIEW);
      return message;
    });

    host.commands.register("hello.clear", () => {
      greetings.length = 0;
      host.views.refresh(VIEW);
    });

    host.views.registerTreeDataProvider(VIEW, {
      async getChildren(parent): Promise<TreeItem[]> {
        if (parent === "history") {
          return greetings.map((label, i) => ({ id: `history/${i}`, label, icon: "comment" }));
        }
        if (parent === "settings") {
          const values = await Promise.all(SETTINGS.map((key) => host.config.get(key)));
          return SETTINGS.map((key, i) => ({
            id: `settings/${key}`,
            label: key,
            description: JSON.stringify(values[i]),
            icon: "gear",
          }));
        }
        return [
          {
            id: "greet",
            label: "Greet",
            description: "run hello.greet",
            icon: "play",
            command: { command: "hello.greet" },
          },
          {
            id: "manifest",
            label: "Open manifest",
            description: "skele-plugin.json",
            icon: "file",
            command: { command: "workbench.openFile", args: [join(import.meta.dirname, "../skele-plugin.json")] },
          },
          { id: "history", label: "History", description: String(greetings.length), collapsible: true },
          { id: "settings", label: "Effective settings", collapsible: true },
        ];
      },
    });

    host.config.onDidChange(() => host.views.refresh(VIEW));
    host.documents.onDidSave(({ uri }) => host.terminal.write(`saved ${uri}\r\n`));
  },
});
