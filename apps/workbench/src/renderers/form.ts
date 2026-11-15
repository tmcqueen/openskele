// Generic form generated from JSON Schemas. It picks a control per schema and
// reports edits; validation is the host's job, so a rejected edit comes back
// as an error to show next to the field.

import { h } from "../dom";

export interface JsonSchema {
  type?: string | string[];
  enum?: unknown[];
  title?: string;
  description?: string;
  default?: unknown;
  minimum?: number;
  maximum?: number;
}

export interface FormField {
  key: string;
  schema: JsonSchema;
  /** Value to show. */
  value: unknown;
  /** Whether `value` is an override that can be reset. */
  modified: boolean;
}

export interface FormOptions {
  fields: FormField[];
  /** Rejects with a message when the host refuses the value. */
  change(key: string, value: unknown): Promise<unknown>;
  reset(key: string): Promise<unknown>;
}

interface Control {
  element: HTMLElement;
  /** Throws when the input cannot be read as a value at all. */
  read(): unknown;
}

function control(field: FormField): Control {
  const { schema, value } = field;
  const type = Array.isArray(schema.type) ? schema.type.find((t) => t !== "null") : schema.type;

  if (schema.enum) {
    const choices = schema.enum;
    const select = h("select", {}, ...choices.map((choice) => h("option", {}, String(choice))));
    select.selectedIndex = choices.findIndex((choice) => choice === value);
    return { element: select, read: () => choices[select.selectedIndex] };
  }
  if (type === "boolean") {
    const checkbox = h("input", { type: "checkbox", checked: value === true });
    return { element: checkbox, read: () => checkbox.checked };
  }
  if (type === "integer" || type === "number") {
    const input = h("input", { type: "number", value: value == null ? "" : String(value) });
    if (schema.minimum !== undefined) input.min = String(schema.minimum);
    if (schema.maximum !== undefined) input.max = String(schema.maximum);
    input.step = type === "integer" ? "1" : "any";
    return {
      element: input,
      read: () => {
        if (input.value.trim() === "" || Number.isNaN(input.valueAsNumber)) throw new Error("Enter a number.");
        return input.valueAsNumber;
      },
    };
  }
  if (type === "string") {
    const input = h("input", { type: "text", value: typeof value === "string" ? value : "" });
    return { element: input, read: () => input.value };
  }
  // Objects, arrays and untyped settings are edited as JSON.
  const area = h("textarea", { rows: 4, value: JSON.stringify(value ?? null, null, 2), spellcheck: false });
  return {
    element: area,
    read: () => {
      try {
        return JSON.parse(area.value);
      } catch {
        throw new Error("Enter valid JSON.");
      }
    },
  };
}

export function createForm(options: FormOptions): HTMLElement {
  const form = h("div", { class: "form" });
  for (const field of options.fields) {
    const { element, read } = control(field);
    const id = `field-${field.key}`;
    element.id = id;
    const error = h("div", { class: "form-error", role: "alert" });

    const attempt = async (action: () => Promise<unknown>) => {
      try {
        await action();
        error.textContent = "";
      } catch (problem) {
        error.textContent = problem instanceof Error ? problem.message : String(problem);
      }
    };
    element.addEventListener("change", () => attempt(async () => options.change(field.key, read())));

    const reset = h("button", { class: "form-reset", type: "button", title: "Reset this setting" }, "Reset");
    reset.addEventListener("click", () => attempt(() => options.reset(field.key)));

    form.append(
      h(
        "div",
        { class: field.modified ? "form-field modified" : "form-field" },
        h("label", { class: "form-label", htmlFor: id }, field.schema.title ?? field.key),
        field.schema.description && h("div", { class: "form-description" }, field.schema.description),
        h("div", { class: "form-control" }, element, field.modified && reset),
        error,
      ),
    );
  }
  return form;
}
