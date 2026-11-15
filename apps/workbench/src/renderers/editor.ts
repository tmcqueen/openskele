// Monaco as a view onto a document buffer the host owns. Edits are pushed to
// the buffer; saving is a host operation so plugins hear about it.

import * as monaco from "monaco-editor/editor/editor.api";
import "monaco-editor/basic-languages/monaco.contribution";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";

import { h } from "../dom";
import { commands } from "../host";
import { toast } from "../toast";

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };

const SYNC_DELAY_MS = 150;

function languageFor(path: string): string {
  const extension = path.slice(path.lastIndexOf("."));
  const language = monaco.languages.getLanguages().find((l) => l.extensions?.includes(extension));
  return language?.id ?? "plaintext";
}

export function retheme(theme: string): void {
  monaco.editor.setTheme(theme === "light" ? "vs" : "vs-dark");
}

export function createEditor(path: string, theme: string): { element: HTMLElement; dispose(): void } {
  const element = h("div", { class: "editor" });
  let editor: monaco.editor.IStandaloneCodeEditor | undefined;
  let disposed = false;

  commands.documentOpen(path).then(
    (document) => {
      if (disposed) return;
      editor = monaco.editor.create(element, {
        value: document.text,
        language: languageFor(document.path),
        theme: theme === "light" ? "vs" : "vs-dark",
        automaticLayout: true,
        minimap: { enabled: false },
      });
      const model = editor.getModel()!;

      // Coalesce keystrokes, but never let a save run ahead of pending text.
      let timer: number | undefined;
      let synced = Promise.resolve();
      const push = () => {
        clearTimeout(timer);
        timer = undefined;
        synced = synced
          .then(() => commands.documentSetText(document.uri, model.getValue()))
          .then(
            () => undefined,
            (error) => toast("error", String(error)),
          );
        return synced;
      };
      model.onDidChangeContent(() => {
        clearTimeout(timer);
        timer = window.setTimeout(push, SYNC_DELAY_MS);
      });
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        void (timer === undefined ? synced : push())
          .then(() => commands.documentSave(document.uri))
          .catch((error) => toast("error", String(error)));
      });
    },
    (error) => element.append(h("div", { class: "tree-error" }, String(error))),
  );

  return {
    element,
    dispose: () => {
      disposed = true;
      editor?.getModel()?.dispose();
      editor?.dispose();
    },
  };
}
