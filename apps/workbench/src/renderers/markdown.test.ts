// Agent text is markdown from a model: it must render as inert content.

import { expect, test } from "bun:test";

import { renderMarkdown } from "./markdown";

function html(source: string): HTMLElement {
  const host = document.createElement("div");
  host.append(renderMarkdown(source));
  return host;
}

test("script tags do not survive", () => {
  const host = html("<script>window.x = 1</script>hi");
  expect(host.querySelector("script")).toBeNull();
});

test("raw HTML is shown as text, not parsed", () => {
  const host = html('<img src="x" onerror="alert(1)">');
  expect(host.querySelector("img")).toBeNull();
  expect(host.textContent).toContain("<img");
});

test("markdown images cannot carry handlers", () => {
  const host = html("![a](x onerror=alert(1))");
  for (const element of host.querySelectorAll("*")) expect(element.getAttribute("onerror")).toBeNull();
});

test("javascript links lose their href", () => {
  const host = html("[click](javascript:alert(1))");
  for (const link of host.querySelectorAll("a")) expect(link.getAttribute("href") ?? "").not.toStartWith("javascript:");
});

test("ordinary markdown renders", () => {
  const host = html("**bold** and `code`\n\n- one\n- two");
  expect(host.querySelector("strong")?.textContent).toBe("bold");
  expect(host.querySelector("code")?.textContent).toBe("code");
  expect(host.querySelectorAll("li").length).toBe(2);
});

test("links never navigate the workbench", () => {
  const host = html("[site](https://example.com)");
  const link = host.querySelector("a")!;
  expect(link.getAttribute("rel")).toBe("noopener noreferrer");
  const click = new MouseEvent("click", { bubbles: true, cancelable: true });
  link.dispatchEvent(click);
  expect(click.defaultPrevented).toBe(true);
});
